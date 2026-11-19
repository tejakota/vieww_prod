#!/usr/bin/env python3
"""Build the Soma health database v2 — adds the semantic and graph layers.

Extends build_db.py's output with two sections:

  Section 4 (terms):  the vector layer — one line per term:
      term \x1f idf \x1f idx:tf,idx:tf;...
      (postings list the topics whose title/description contain the term,
       with the term's frequency in that topic's weighted text)
      First line is special: `#norms \x1f n0,n1,n2,...` — each topic's
      TF-IDF vector length, so cosine similarity is one multiply at
      query time instead of a corpus scan.
  Section 5 (graph):  the anatomy graph — one line per organ:
      organ_key \x1f neighbor:weight,neighbor:weight;...
      (edges are corpus co-mentions: a topic assigned to organ A whose
       body text also mentions organ B contributes an A-B edge. Weight
       is the co-mention count normalized to the row's max.)

The tokenizer here and the one in soma's `search.rs` are the same
algorithm — lowercase, split on non-alphanumeric, stopwords, and a small
suffix stemmer — and both are covered by matching test vectors. Change
one, change both.
"""
import json
import math
import re
from pathlib import Path

# Location-independent: run from anywhere, output to the shipping asset.
HERE = Path(__file__).resolve().parent
SRC = HERE / "soma_db_v1.txt"
OUT = (HERE / "../assets/db/soma_db.txt").resolve()

# --------------------------------------------------------------------------
# The tokenizer (mirrors search.rs::tokenize)
# --------------------------------------------------------------------------

STOP = {
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do",
    "does", "for", "from", "had", "has", "have", "how", "i", "if", "in",
    "into", "is", "it", "its", "may", "of", "on", "or", "should", "so",
    "than", "that", "the", "their", "them", "there", "they", "this", "to",
    "was", "we", "were", "what", "when", "where", "which", "who", "why",
    "will", "with", "you", "your", "not", "no", "also", "more", "most",
    "other", "some", "such", "than", "then", "these", "those", "via",
    "about", "after", "all", "any", "been", "before", "being", "both",
    "could", "each", "few", "had", "having", "he", "her", "his", "him",
    "me", "my", "our", "out", "over", "same", "she", "too", "under",
    "up", "down", "very", "while", "within", "without", "cause", "causes",
    "called", "common", "commonly", "like", "make", "makes", "made",
    "people", "person", "see", "seen", "symptom", "symptoms", "test",
    "tests", "tested", "treatment", "treatments", "care", "health",
    "condition", "conditions", "disease", "diseases", "learn", "know",
    "find", "get", "gets", "getting", "need", "needs", "used", "using",
    "type", "types", "ways", "way", "including", "include", "includes",
    "well", "many", "much", "even", "just", "also", "often", "usually",
}

TOKEN_RE = re.compile(r"[a-z0-9]+")


def stem(t: str) -> str:
    """The same suffix rules search.rs applies, in the same order.

    The `es` rules are sibilant-aware: `es` is a real suffix only after
    ch/sh/x/z/ss (watches, dishes, boxes, buzzes, classes), and the whole
    -aches family is "-ache" + "s". Everywhere else the e belongs to the
    word ("stones" -> "stone", not "ston") — the singular and the plural
    must meet in one key, or a query for one can never find the other.
    """
    if len(t) > 4 and t.endswith("ies"):
        return t[:-3] + "y"
    if len(t) > 5 and t.endswith("ing"):
        return t[:-3]
    if len(t) > 4 and t.endswith("ed"):
        return t[:-2]
    if len(t) > 4 and t.endswith("aches"):
        return t[:-1]
    if len(t) > 4 and (t.endswith("sses") or t.endswith("ches") or t.endswith("shes")):
        return t[:-2]
    if len(t) > 3 and t.endswith("es") and t[-3] in "xz":
        return t[:-2]
    if len(t) > 3 and t.endswith("s") and not t.endswith("ss"):
        return t[:-1]
    return t


def tokenize(text: str) -> list:
    out = []
    for raw in TOKEN_RE.findall(text.lower()):
        if raw in STOP or len(raw) < 2:
            continue
        t = stem(raw)
        if t and t not in STOP and len(t) >= 2:
            out.append(t)
    return out


# --------------------------------------------------------------------------
# Graph extraction: organ co-mentions over full-article bodies
# --------------------------------------------------------------------------

ORGAN_TERMS = {
    # key: terms whose mention in another topic's body links it to this organ
    "brain": ["brain", "cerebrum", "cerebellum", "neurologist"],
    "spine": ["spine", "spinal", "vertebra", "vertebrae", "backbone"],
    "heart": ["heart", "cardiac", "coronary", "aorta", "artery", "arteries", "ventricle"],
    "lungs": ["lung", "lungs", "pulmonary", "bronchi", "trachea", "respiratory"],
    "liver": ["liver", "hepatic", "bile"],
    "kidneys": ["kidney", "kidneys", "renal", "nephron", "dialysis"],
    "stomach": ["stomach", "gastric", "esophagus"],
    "intestines": ["intestine", "intestines", "bowel", "colon", "rectum", "gut"],
    "pancreas": ["pancreas", "pancreatic", "insulin"],
    "liver_gall": ["gallbladder", "gallstone", "gall bladder", "bile duct"],
    "spleen": ["spleen", "splenic", "lymph node", "lymph nodes"],
    "thyroid": ["thyroid", "parathyroid", "endocrine"],
    "breasts": ["breast", "breasts", "mammary", "nipple"],
    "reproductive": ["prostate", "testicle", "testicles", "ovary", "ovaries",
                     "uterus", "uterine", "cervix", "vagina", "penis"],
    "bladder": ["bladder", "urethra", "ureter", "urine", "urinary", "urination"],
    "skin": ["skin", "dermatologist", "epidermis", "rash", "hair follicle"],
    "bones": ["bone", "bones", "skeleton", "skeletal", "joint", "joints",
              "cartilage", "femur", "tibia", "spine"],
    "muscles": ["muscle", "muscles", "tendon", "tendons", "ligament", "ligaments"],
    "eyes": ["eye", "eyes", "retina", "cornea", "pupil", "optic nerve", "vision"],
    "ears": ["ear", "ears", "cochlea", "eardrum", "hearing", "tinnitus"],
    "mouth": ["tooth", "teeth", "tongue", "gum", "gums", "saliva", "jaw", "dental"],
    "blood": ["blood", "plasma", "platelet", "platelets", "red blood cell",
              "white blood cell", "hemoglobin", "clot", "clots", "circulatory"],
    "nerves": ["nerve", "nerves", "neuron", "neurons", "neurological",
               "central nervous", "spinal cord"],
    "head": ["skull", "facial", "face", "headache", "migraine", "neck"],
    "whole": [],
}

# Sentence-level proximity: mentions count only if the term appears in the
# topic's body text at all. Simple presence, no weighting by distance.


def build_graph(topic_organ, bodies, organ_keys):
    """co-mention counts between a topic's organ and organs its body mentions."""
    # word-boundary regexes, precompiled: a plain substring test would make
    # "years" mention "ears" and "heartburn" mention "heart"
    term_rx = {
        okey: [re.compile(r"\b" + re.escape(term).replace(r"\ ", r"\s") + r"\b", re.I)
               for term in terms]
        for okey, terms in ORGAN_TERMS.items()
    }
    edges = {k: {} for k in organ_keys}
    for idx, organ in topic_organ.items():
        body = bodies.get(idx)
        if not body:
            continue
        for okey, rxs in term_rx.items():
            if okey == organ:
                continue
            for rx in rxs:
                if rx.search(body):
                    edges[organ][okey] = edges[organ].get(okey, 0) + 1
                    break
    # normalize each row to [0, 1] by its max, keep the top 8 neighbors
    lines = []
    for k in organ_keys:
        row = edges.get(k, {})
        if not row:
            lines.append(f"{k}\x1f")
            continue
        mx = max(row.values())
        top = sorted(row.items(), key=lambda kv: -kv[1])[:8]
        cells = ";".join(f"{n}:{w / mx:.3f}" for n, w in top)
        lines.append(f"{k}\x1f{cells}")
    return lines


# --------------------------------------------------------------------------
# The term index
# --------------------------------------------------------------------------

def build_terms(topic_lines, n_topics):
    """TF-IDF postings over title (weight 3) + description (weight 1)."""
    tf = {}  # term -> {topic_idx: weight}
    df = {}  # term -> number of topics containing it
    for idx, line in enumerate(topic_lines):
        fields = line.split("\x1f")
        title = fields[2] if len(fields) > 2 else ""
        desc = fields[3] if len(fields) > 3 else ""
        counts = {}
        for t in tokenize(title):
            counts[t] = counts.get(t, 0) + 3.0
        for t in tokenize(desc):
            counts[t] = counts.get(t, 0) + 1.0
        for t, c in counts.items():
            tf.setdefault(t, {})[idx] = c
            df[t] = df.get(t, 0) + 1

    n = n_topics
    # drop singleton terms: they cannot make two topics similar, and they
    # are most of the long tail
    terms = sorted(t for t in tf if df[t] >= 2)
    idf = {t: math.log((n + 1) / (df[t] + 0.5)) for t in terms}

    # per-topic vector norms for cosine at query time
    norms = [0.0] * n
    for t in terms:
        w_idf = idf[t]
        for idx, c in tf[t].items():
            norms[idx] += (c * w_idf) ** 2
    norms = [math.sqrt(v) for v in norms]

    lines = ["#norms\x1f" + ",".join(f"{v:.4f}" for v in norms)]
    for t in terms:
        postings = tf[t]
        cells = ";".join(f"{idx}:{c:.0f}" for idx, c in sorted(postings.items()))
        lines.append(f"{t}\x1f{idf[t]:.4f}\x1f{cells}")
    return lines, len(terms)


def main():
    raw = SRC.read_text(encoding="utf-8")
    parts = raw.split("\n\x1d\n")
    topic_lines = [l for l in parts[0].splitlines() if l]
    organ_lines = [l for l in parts[1].splitlines() if l]
    body_records = [r for r in parts[2].split("\x1e") if r]

    # topic -> organ assignment
    topic_organ = {}
    bodies = {}
    for line in topic_lines:
        f = line.split("\x1f")
        try:
            idx = topic_lines.index(line)
        except ValueError:
            continue
    for idx, line in enumerate(topic_lines):
        f = line.split("\x1f")
        oid = int(f[1]) if len(f) > 1 and f[1] != "-1" else -1
        if oid >= 0:
            topic_organ[idx] = oid
    for rec in body_records:
        f = rec.split("\x1f", 1)
        if len(f) == 2 and f[0].isdigit():
            bodies[int(f[0])] = f[1]

    organ_keys = []
    for line in organ_lines:
        f = line.split("\x1f")
        if len(f) >= 3:
            organ_keys.append(f[2])

    print(f"{len(topic_lines)} topics, {len(bodies)} bodies, {len(organ_keys)} organs")

    term_lines, vocab = build_terms(topic_lines, len(topic_lines))
    print(f"term index: {vocab} terms, {len(term_lines) - 1} lines")

    # graph needs organ KEYS for topic organs (index -> key)
    key_by_idx = {i: k for i, k in enumerate(organ_keys)}
    topic_organ_keys = {i: key_by_idx[o] for i, o in topic_organ.items() if o < len(organ_keys)}
    graph_lines = build_graph(topic_organ_keys, bodies, organ_keys)

    out = raw + "\n\x1d\n" + "\n".join(term_lines) + "\n\x1d\n" + "\n".join(graph_lines)
    OUT.write_text(out, encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size / 1e6:.1f} MB)")
    # sanity: show a few graph lines
    for line in graph_lines[:6]:
        print("  ", line[:100])


if __name__ == "__main__":
    import sys
    if "--selftest" in sys.argv:
        # Mirrored with search.rs's `tokenizer_matches_the_builder`: the
        # same text must tokenize identically on both sides.
        vectors = {
            "Heart Attacks: Chest Pain": ["heart", "attack", "chest", "pain"],
            "Sleep Studies": ["sleep", "study"],
            "running": ["runn"],
            "medicated": ["medicat"],
            "the a of is it": [],
            "kidney-stone (renal)": ["kidney", "stone", "renal"],
            # The plural fix: singular and plural must meet in one key.
            "kidney stones": ["kidney", "stone"],
            "headaches": ["headache"],
            "muscles": ["muscle"],
            "watches": ["watch"],
            "boxes": ["box"],
            "houses": ["house"],
        }
        for text, want in vectors.items():
            got = tokenize(text)
            assert got == want, f"{text!r}: {got} != {want}"
        print(f"tokenizer self-test: {len(vectors)} vectors ok")
    else:
        main()
