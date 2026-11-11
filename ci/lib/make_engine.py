"""Write .github/workflows/_job.yml, the engine that runs planned jobs.

    python3 ci/lib/make_engine.py           rewrite it
    python3 ci/lib/make_engine.py --check   fail if it is out of date

The engine has two jobs with the same steps: `run` (read-only token) and
`write` (may push, release and attest; waits for the `release`
environment's approval when the phase publishes). Its job env has one
line per secret any ci.toml names, and each job sees only the secrets its
own entry lists, so the secret list is generated: add a secret to a
ci.toml, then run this (ci/projects sync does).
"""
import glob
import os
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, ".github", "workflows", "_job.yml")
UPLOADS, DOWNLOADS = 4, 2


def all_secrets():
    names = set()
    files = glob.glob(os.path.join(ROOT, "**", "ci.toml"), recursive=True) + glob.glob(os.path.join(ROOT, "ci", "kinds", "*.toml"))
    for f in files:
        if "/target/" in f or "/node_modules/" in f:
            continue
        with open(f, "rb") as fh:
            for j in tomllib.load(fh).get("job", []):
                names.update(j.get("secrets", []))
    return sorted(names)


PY = '"${CI_PY:-python3}"'


def steps(kind):
    s = []
    a = s.append
    a("""      - name: Long paths (Windows)
        if: runner.os == 'Windows'
        shell: powershell
        run: git config --system core.longpaths true""")
    a("""      - uses: actions/checkout@v4
        with:
          fetch-depth: ${{ matrix.checkout.fetch_depth }}
          persist-credentials: ${{ matrix.checkout.persist }}
          clean: ${{ matrix.checkout.clean }}
          sparse-checkout: ${{ matrix.checkout.sparse }}""")
    a("""      - name: Python for the runner (GitHub's machines)
        id: py
        if: runner.environment == 'github-hosted'
        uses: actions/setup-python@v5
        with:
          python-version: '3.12'
          update-environment: false""")
    a("""      - name: Plan this job
        id: prep
        shell: bash
        env:
          CI_PY: ${{ steps.py.outputs.python-path }}
        run: |
          [ -n "$CI_PY" ] && echo "CI_PY=$CI_PY" >> "$GITHUB_ENV"
          """ + PY + """ ci/lib/prepare.py""")
    go = "steps.prep.outputs.go == 'true'"
    a(f"""      - if: {go} && steps.prep.outputs.java != ''
        uses: actions/setup-java@v4
        with:
          distribution: ${{{{ steps.prep.outputs.java_dist }}}}
          java-version: ${{{{ steps.prep.outputs.java }}}}""")
    a(f"""      - if: {go} && steps.prep.outputs.node != ''
        uses: actions/setup-node@v4
        with:
          node-version: ${{{{ steps.prep.outputs.node }}}}""")
    a(f"""      - if: {go} && steps.prep.outputs.gradle != ''
        uses: gradle/actions/setup-gradle@v4
        with:
          gradle-version: ${{{{ steps.prep.outputs.gradle }}}}""")
    a(f"""      - if: {go} && steps.prep.outputs.tools != ''
        uses: taiki-e/install-action@v2
        with:
          tool: ${{{{ steps.prep.outputs.tools }}}}""")
    a(f"""      - if: {go} && steps.prep.outputs.rust_cache != ''
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: ${{{{ steps.prep.outputs.rust_cache }}}}
          key: ${{{{ steps.prep.outputs.rust_cache_key }}}}""")
    a(f"""      - if: {go} && steps.prep.outputs.cache_key != ''
        uses: actions/cache@v4
        with:
          path: ${{{{ steps.prep.outputs.cache_path }}}}
          key: ${{{{ steps.prep.outputs.cache_key }}}}
          restore-keys: ${{{{ steps.prep.outputs.cache_restore }}}}""")
    a(f"""      - name: AWS credentials (OIDC role)
        if: {go} && steps.prep.outputs.aws != '' && env.CI_HAS_AWS_ROLE == 'true'
        uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{{{ secrets.AWS_ROLE_ARN }}}}
          aws-region: ${{{{ steps.prep.outputs.aws }}}}""")
    a(f"""      - name: AWS credentials (access keys)
        if: {go} && steps.prep.outputs.aws != '' && env.CI_HAS_AWS_ROLE != 'true'
        uses: aws-actions/configure-aws-credentials@v4
        with:
          aws-access-key-id: ${{{{ secrets.AWS_ACCESS_KEY_ID }}}}
          aws-secret-access-key: ${{{{ secrets.AWS_SECRET_ACCESS_KEY }}}}
          aws-region: ${{{{ steps.prep.outputs.aws }}}}""")
    for i in range(DOWNLOADS):
        a(f"""      - if: {go} && steps.prep.outputs.dl{i} == 'true'
        uses: actions/download-artifact@v4
        continue-on-error: ${{{{ steps.prep.outputs.dl{i}_soft == 'true' }}}}
        with:
          name: ${{{{ steps.prep.outputs.dl{i}_name }}}}
          pattern: ${{{{ steps.prep.outputs.dl{i}_pattern }}}}
          path: ${{{{ steps.prep.outputs.dl{i}_path }}}}
          merge-multiple: ${{{{ steps.prep.outputs.dl{i}_merge == 'true' }}}}""")
    a(f"""      - name: ${{{{ matrix.title }}}}
        id: main
        if: {go}
        shell: bash
        env:
          CI_PART: main
        run: |
          {PY} ci/lib/run.py "${{{{ fromJSON(matrix.job).file }}}}"
""".rstrip("\n"))
    a(f"""      - name: Android emulator
        if: {go} && steps.prep.outputs.emulator == 'true'
        uses: reactivecircus/android-emulator-runner@v2
        env:
          CI_PART: emulator
          CI_JOB_FILE: ${{{{ fromJSON(matrix.job).file }}}}
          ANDROID_NDK_HOME: ${{{{ env.ANDROID_NDK_LATEST_HOME }}}}
        with:
          api-level: ${{{{ steps.prep.outputs.emu_api_level }}}}
          arch: ${{{{ steps.prep.outputs.emu_arch }}}}
          target: ${{{{ steps.prep.outputs.emu_target }}}}
          emulator-options: ${{{{ steps.prep.outputs.emu_emulator_options }}}}
          script: python3 ci/lib/run.py "$CI_JOB_FILE\"""")
    a(f"""      - name: Build provenance (Sigstore, keyless)
        if: {go} && steps.prep.outputs.attest != ''
        uses: actions/attest-build-provenance@v2
        with:
          subject-path: ${{{{ steps.prep.outputs.attest }}}}""")
    a(f"""      - name: After
        if: always() && {go} && steps.prep.outputs.has_final == 'true'
        shell: bash
        env:
          CI_PART: final
        run: |
          {PY} ci/lib/run.py "${{{{ fromJSON(matrix.job).file }}}}"
""".rstrip("\n"))
    for i in range(UPLOADS):
        a(f"""      - if: always() && {go} && steps.prep.outputs.up{i}_name != '' && env.CI_UPLOAD_{i} == 'yes'
        uses: actions/upload-artifact@v4
        with:
          name: ${{{{ steps.prep.outputs.up{i}_name }}}}
          path: ${{{{ steps.prep.outputs.up{i}_path }}}}
          if-no-files-found: ${{{{ steps.prep.outputs.up{i}_ifnf }}}}
          retention-days: ${{{{ steps.prep.outputs.up{i}_days }}}}
          include-hidden-files: ${{{{ steps.prep.outputs.up{i}_hidden == 'true' }}}}""")
    return "\n\n".join(s)


def job(kind, secrets):
    env = ["      CI_INPUTS: ${{ matrix.inputs }}", "      CI_MATRIX: ${{ matrix.matrix }}",
           "      CI_JOB: ${{ matrix.job }}", "      CI_PROJECT: ${{ matrix.project }}",
           "      CI_VARS: ${{ inputs.vars }}", "      CI_GITHUB_TOKEN: ${{ github.token }}",
           "      CI_SECRET_NAMES: ${{ matrix.secret_list }}",
           "      CI_HAS_AWS_ROLE: ${{ secrets.AWS_ROLE_ARN != '' }}"]
    for n in secrets:
        env.append(f"      {n}: ${{{{ contains(matrix.secret_names, ',{n},') && secrets.{n} || '' }}}}")
    if kind == "run":
        head = """  run:
    if: inputs.jobs != '[]'
    permissions:
      contents: read
      actions: read
      id-token: write"""
    else:
        head = """  approve:
    if: inputs.write_jobs != '[]' && inputs.approval
    runs-on: ubuntu-latest
    environment: release
    steps:
      - run: echo "Approved for publishing by the release environment's reviewer."

  write:
    needs: approve
    if: ${{ !cancelled() && inputs.write_jobs != '[]' && needs.approve.result != 'failure' }}
    permissions:
      contents: write
      actions: read
      id-token: write
      attestations: write"""
    src = "inputs.jobs" if kind == "run" else "inputs.write_jobs"
    return f"""{head}
    name: ${{{{ matrix.title }}}}
    runs-on: ${{{{ matrix.runs_on }}}}
    timeout-minutes: ${{{{ matrix.timeout }}}}
    continue-on-error: ${{{{ matrix.continue_on_error }}}}
    strategy:
      fail-fast: false
      matrix:
        include: ${{{{ fromJSON({src}) }}}}
    env:
{chr(10).join(env)}
    steps:
{steps(kind)}
"""


def render():
    secrets = all_secrets()
    return f"""# GENERATED by ci/lib/make_engine.py — edit that, then run ci/projects sync.
#
# The engine: runs the jobs a numbered workflow's plan picked for one phase.
# Every entry is a job from some project's ci.toml (or its kind's): the
# engine checks out, sets up what the entry asks for, runs its job file
# through ci/lib/run.py and uploads its artifacts. It knows kinds of setup,
# never projects. `write` holds the entries that push, release or deploy;
# in a publishing phase it waits for the `release` environment's approval.
name: _job

on:
  workflow_call:
    inputs:
      jobs:
        type: string
        default: '[]'
      write_jobs:
        type: string
        default: '[]'
      approval:
        type: boolean
        default: false
      vars:
        type: string
        default: '{{}}'

permissions: {{}}

jobs:
{job("run", secrets)}
{job("write", secrets)}"""


def main():
    text = render()
    if "--check" in sys.argv:
        try:
            cur = open(OUT, encoding="utf-8").read()
        except OSError:
            cur = ""
        if cur != text:
            print("::error::.github/workflows/_job.yml is out of date (a ci.toml names a secret it does not pass?) — run ci/projects sync")
            sys.exit(1)
        return
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {os.path.relpath(OUT, ROOT)} ({len(all_secrets())} secrets)")


if __name__ == "__main__":
    main()
