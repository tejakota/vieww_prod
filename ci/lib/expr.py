"""GitHub Actions expressions, evaluated outside GitHub.

Job files under a project's ``ci/jobs/`` keep the expressions their steps
were written with (``${{ matrix.os }}``, ``if: failure()``, ...). The step
runner (``run.py``) and the planner (``ci/projects``) evaluate them here
with GitHub's rules: loose equality that ignores case, ``&&``/``||`` that
return an operand, ``contains``/``startsWith``/``format``/``fromJSON`` and
friends, and the status functions.

    evaluate("matrix.hosted && env.SCOPE != 'screenshots'", ctx)
    interpolate("cargo test -p ${{ matrix.package }}", ctx)

``ctx`` maps a context name (``inputs``, ``matrix``, ``env``, ``github``,
``runner``, ``steps``, ``needs``, ``secrets``, ``vars``, ``job``) to a dict
or to a callable that takes the property name. ``ctx["__status__"]`` is
the job status so far (``success`` or ``failure``) for ``success()`` and
friends.

Standard library only: this runs on self-hosted machines too.
"""
import fnmatch
import glob
import hashlib
import json
import math
import os
import re

__all__ = ["evaluate", "interpolate", "truthy", "to_str", "ExprError"]


class ExprError(ValueError):
    pass


_TOKEN = re.compile(r"""
    \s*(?:
      (?P<num>-?(?:0x[0-9a-fA-F]+|\d+(?:\.\d+)?(?:[eE][-+]?\d+)?))
    | (?P<str>'(?:[^']|'')*')
    | (?P<op>&&|\|\||==|!=|<=|>=|[!<>()\[\],.*])
    | (?P<name>[A-Za-z_][A-Za-z0-9_-]*)
    )""", re.X)


def _tokens(text):
    pos, out = 0, []
    text = text.strip()
    while pos < len(text):
        m = _TOKEN.match(text, pos)
        if not m or m.end() == pos:
            raise ExprError(f"cannot read expression at {text[pos:]!r} in {text!r}")
        pos = m.end()
        kind = m.lastgroup
        val = m.group(kind)
        if kind == "num":
            val = float(int(val, 16)) if val.lower().startswith(("0x", "-0x")) else float(val)
        elif kind == "str":
            val = val[1:-1].replace("''", "'")
        out.append((kind, val))
    out.append(("end", None))
    return out


# ── coercion ─────────────────────────────────────────────────────────────

def truthy(v):
    if v is None or v is False:
        return False
    if v is True:
        return True
    if isinstance(v, (int, float)):
        return not (v == 0 or (isinstance(v, float) and math.isnan(v)))
    if isinstance(v, str):
        return v != ""
    return True  # arrays and objects


def _num(v):
    if v is None:
        return 0.0
    if isinstance(v, bool):
        return 1.0 if v else 0.0
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, str):
        s = v.strip()
        if s == "":
            return 0.0
        try:
            return float(int(s, 16)) if s.lower().startswith("0x") else float(s)
        except ValueError:
            return math.nan
    return math.nan


def to_str(v):
    if v is None:
        return ""
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, float):
        return str(int(v)) if v.is_integer() else repr(v)
    if isinstance(v, (dict, list)):
        return json.dumps(v, indent=2)
    return str(v)


def _eq(a, b):
    if type(a) is type(b) or (isinstance(a, (int, float)) and isinstance(b, (int, float))
                              and not isinstance(a, bool) and not isinstance(b, bool)):
        if isinstance(a, str):
            return a.lower() == b.lower()
        if isinstance(a, (dict, list)):
            return a is b
        return a == b
    if isinstance(a, (dict, list)) or isinstance(b, (dict, list)):
        return False
    return _num(a) == _num(b)


def _cmp(a, b, op):
    if isinstance(a, str) and isinstance(b, str):
        x, y = a.lower(), b.lower()
    else:
        x, y = _num(a), _num(b)
        if math.isnan(x) or math.isnan(y):
            return False
    return {"<": x < y, "<=": x <= y, ">": x > y, ">=": x >= y}[op]


# ── functions ────────────────────────────────────────────────────────────

def _contains(hay, needle):
    if isinstance(hay, list):
        return any(_eq(x, needle) for x in hay)
    return to_str(needle).lower() in to_str(hay).lower()


def _format(fmt, *args):
    fmt = to_str(fmt)
    out, i = [], 0
    while i < len(fmt):
        c = fmt[i]
        if c == "{" and fmt[i:i + 2] == "{{":
            out.append("{"); i += 2; continue
        if c == "}" and fmt[i:i + 2] == "}}":
            out.append("}"); i += 2; continue
        if c == "{":
            j = fmt.index("}", i)
            out.append(to_str(args[int(fmt[i + 1:j])]))
            i = j + 1
            continue
        out.append(c); i += 1
    return "".join(out)


def _hash_files(*patterns):
    root = os.environ.get("GITHUB_WORKSPACE") or os.getcwd()
    files = set()
    for p in patterns:
        neg = p.startswith("!")
        hits = {f for f in glob.glob(os.path.join(root, p.lstrip("!")), recursive=True) if os.path.isfile(f)}
        files = files - hits if neg else files | hits
    if not files:
        return ""
    total = hashlib.sha256()
    for f in sorted(files):
        with open(f, "rb") as fh:
            total.update(hashlib.sha256(fh.read()).digest())
    return total.hexdigest()


def _status(ctx, which):
    st = ctx.get("__status__", "success")
    return {"success": st == "success", "failure": st == "failure",
            "always": True, "cancelled": st == "cancelled"}[which]


FUNCS = {
    "contains": lambda c, a, b: _contains(a, b),
    "startswith": lambda c, a, b: to_str(a).lower().startswith(to_str(b).lower()),
    "endswith": lambda c, a, b: to_str(a).lower().endswith(to_str(b).lower()),
    "format": lambda c, f, *a: _format(f, *a),
    "join": lambda c, a, sep=",": (to_str(sep).join(to_str(x) for x in a) if isinstance(a, list) else to_str(a)),
    "tojson": lambda c, a: json.dumps(a, indent=2),
    "fromjson": lambda c, a: json.loads(to_str(a)),
    "hashfiles": lambda c, *p: _hash_files(*[to_str(x) for x in p]),
    "success": lambda c: _status(c, "success"),
    "failure": lambda c: _status(c, "failure"),
    "always": lambda c: True,
    "cancelled": lambda c: _status(c, "cancelled"),
}


# ── parser (precedence: ! > comparison > == != > && > ||) ───────────────

class _Parser:
    def __init__(self, text, ctx):
        self.t, self.i, self.ctx, self.text = _tokens(text), 0, ctx, text

    def peek(self):
        return self.t[self.i]

    def take(self, kind=None, val=None):
        k, v = self.t[self.i]
        if (kind and k != kind) or (val is not None and v != val):
            raise ExprError(f"expected {val or kind} in {self.text!r}, got {v!r}")
        self.i += 1
        return v

    def parse(self):
        v = self.or_()
        if self.peek()[0] != "end":
            raise ExprError(f"unexpected {self.peek()[1]!r} in {self.text!r}")
        return v

    def or_(self):
        v = self.and_()
        while self.peek() == ("op", "||"):
            self.take()
            r = self.and_()
            v = v if truthy(v) else r
        return v

    def and_(self):
        v = self.eq()
        while self.peek() == ("op", "&&"):
            self.take()
            r = self.eq()
            v = r if truthy(v) else v
        return v

    def eq(self):
        v = self.cmp()
        while self.peek()[0] == "op" and self.peek()[1] in ("==", "!="):
            op = self.take()
            r = self.cmp()
            v = _eq(v, r) if op == "==" else not _eq(v, r)
        return v

    def cmp(self):
        v = self.unary()
        while self.peek()[0] == "op" and self.peek()[1] in ("<", "<=", ">", ">="):
            op = self.take()
            v = _cmp(v, self.unary(), op)
        return v

    def unary(self):
        if self.peek() == ("op", "!"):
            self.take()
            return not truthy(self.unary())
        return self.postfix(self.primary())

    def primary(self):
        k, v = self.peek()
        if k in ("num", "str"):
            self.take()
            return v
        if k == "op" and v == "(":
            self.take()
            r = self.or_()
            self.take("op", ")")
            return r
        if k == "name":
            self.take()
            low = v.lower()
            if low in ("true", "false"):
                return low == "true"
            if low == "null":
                return None
            if self.peek() == ("op", "("):
                self.take()
                args = []
                if self.peek() != ("op", ")"):
                    args.append(self.or_())
                    while self.peek() == ("op", ","):
                        self.take()
                        args.append(self.or_())
                self.take("op", ")")
                if low not in FUNCS:
                    raise ExprError(f"unknown function {v}() in {self.text!r}")
                return FUNCS[low](self.ctx, *args)
            c = self.ctx.get(v, self.ctx.get(low))
            return c
        raise ExprError(f"unexpected {v!r} in {self.text!r}")

    def postfix(self, v):
        while True:
            k, t = self.peek()
            if k == "op" and t == ".":
                self.take()
                nk, name = self.peek()
                if nk == "op" and name == "*":
                    self.take()
                    v = list(v.values()) if isinstance(v, dict) else (v if isinstance(v, list) else [])
                    continue
                name = self.take("name")
                v = _prop(v, name)
            elif k == "op" and t == "[":
                self.take()
                key = self.or_()
                self.take("op", "]")
                v = _index(v, key)
            else:
                return v


def _prop(v, name):
    if callable(v):
        return v(name)
    if isinstance(v, dict):
        if name in v:
            return v[name]
        low = name.lower()
        for k, x in v.items():
            if k.lower() == low:
                return x
        return None
    if isinstance(v, list):  # filtered array: a.*.b
        return [_prop(x, name) for x in v]
    return None


def _index(v, key):
    if isinstance(v, list):
        n = _num(key)
        return v[int(n)] if not math.isnan(n) and 0 <= int(n) < len(v) else None
    return _prop(v, to_str(key))


def evaluate(text, ctx):
    text = text.strip()
    if text.startswith("${{") and text.endswith("}}"):
        text = text[3:-2]
    return _Parser(text, ctx).parse()


_INTERP = re.compile(r"\$\{\{(.*?)\}\}", re.S)


def interpolate(text, ctx):
    """Replace every ${{ expr }} in text with the expression's string."""
    if not isinstance(text, str) or "${{" not in text:
        return text
    return _INTERP.sub(lambda m: to_str(evaluate(m.group(1), ctx)), text)


def condition(text, ctx):
    """A step's or job's `if:`. Like GitHub, a condition that calls no
    status function is implicitly `success() && (...)`."""
    if text is None or (isinstance(text, str) and text.strip() == ""):
        return _status(ctx, "success")
    if isinstance(text, bool):
        return text and _status(ctx, "success")
    body = text.strip()
    if body.startswith("${{") and body.endswith("}}"):
        body = body[3:-2]
    if not re.search(r"\b(success|failure|always|cancelled)\s*\(", body):
        if not _status(ctx, "success"):
            return False
    return truthy(evaluate(body, ctx))


def matches(pattern, path):
    return fnmatch.fnmatch(path, pattern)
