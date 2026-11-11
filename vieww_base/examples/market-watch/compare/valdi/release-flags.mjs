// The Bazel flags Valdi's own CLI passes for a shipped Android app:
//
//   valdi package android --build_config release   (arm64 phone)
//
// Read from the pinned checkout's CLI source rather than copied here, so the
// comparison builds Valdi the way Valdi builds itself at that commit. Fails if
// the CLI no longer declares any of them, rather than building something else.
//
//   node release-flags.mjs VALDI_CHECKOUT    → one flag per line
//
// Mirrors resolveBazelBuildArgs (npm_modules/cli/src/utils/applicationUtils.ts):
// RELEASE_BUILD_FLAGS, then ANDROID_BUILD_FLAGS, then the arm64 defines, then
// --android_platforms for arm64. The JS engine is the workspace's own default
// (.bazelrc: Hermes, as in the template `valdi bootstrap` writes), and the
// runtime-logs and runtime-traces flags stay off, as they are by default.

import fs from 'node:fs';
import path from 'node:path';

const root = process.argv[2];
if (!root) {
  console.error('usage: node release-flags.mjs VALDI_CHECKOUT');
  process.exit(2);
}
const file = path.join(root, 'npm_modules/cli/src/core/constants.ts');
const src = fs.readFileSync(file, 'utf8');

function array(name) {
  const m = src.match(new RegExp(`export const ${name} = (\\[[^\\]]*\\])`));
  if (!m) throw new Error(`release-flags: ${name} is no longer in ${file}`);
  return JSON.parse(m[1].replace(/'/g, '"'));
}

function arm64Platform() {
  const m = src.match(/\[Architecture\.ARM64\]:\s*'([^']+)'/);
  if (!m) throw new Error(`release-flags: no arm64 entry in ANDROID_PLATFORMS in ${file}`);
  return m[1];
}

const flags = [
  ...array('RELEASE_BUILD_FLAGS'),
  ...array('ANDROID_BUILD_FLAGS'),
  // `--define client_repo_arm64=true` is two words in the CLI's array.
  array('ANDROID_ARM64_BUILD_FLAGS').join(' ').replace('--define ', '--define='),
  `--android_platforms=${arm64Platform()}`,
];
console.log(flags.join('\n'));
