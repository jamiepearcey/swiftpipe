#!/usr/bin/env python3
"""Generate starter SwiftPipe MT schemas from Prowide Core's message structure docs.

Source: prowide/prowide-core generated MT classes (Apache-2.0), pinned commit below.
Each class carries a javadoc `scheme` (sequences, tags, options, M/O) and getters whose
`List<>` return type marks repeatable fields. Only the structure is used; schemas are
starter schemas, not licensed SRU imports.

usage: gen-schemas-from-prowide.py <dir-of-MT*.java> <schemas-dir> <structures.json> [--only MT104,MT107] [--samples dir]
Existing schema files are never overwritten.
"""
import json, os, re, sys

COMMIT = "c93572dba6e822efa9a471d506fa4966c73ae84f"
SRC = "src/generated/java/com/prowidesoftware/swift/model/mt/mt{c}xx/MT{n}.java"
CAT = {"1": "payments", "2": "payments", "3": "treasury", "4": "collections",
       "6": "markets", "7": "trade-finance", "8": "travellers-cheques", "9": "cash"}
NAMES = {"20": "transaction_reference", "21": "related_reference", "25": "account_identification",
         "28C": "statement_number", "32A": "value_date_currency_amount", "32B": "currency_amount",
         "30": "date", "52": "ordering_institution", "53": "senders_correspondent",
         "54": "receivers_correspondent", "56": "intermediary", "57": "account_with_institution",
         "58": "beneficiary_institution", "59": "beneficiary_customer", "50": "ordering_customer",
         "70": "details", "71A": "details_of_charges", "72": "sender_to_receiver_information",
         "77B": "regulatory_reporting", "79": "narrative", "11S": "mt_and_date_of_original_message",
         "13C": "time_indication", "13D": "date_time_indication", "23E": "instruction_code",
         "26T": "transaction_type_code", "33B": "instructed_amount", "36": "exchange_rate",
         "51A": "sending_institution", "75": "queries", "76": "answers", "77A": "narrative_77a"}

FIELD = re.compile(r'<li class="field">Field (\d{2,3})\s*([A-Z0-9,]*)\s*\((M|O)\)( \(repetitive\))?</li>')
SEQ = re.compile(r'<li class="sequence">\s*Sequence (\w+) \((M|O)\)( \(repetitive\))?<ul>')


def parse_scheme(text):
    """-> (list of sequences in document order, nested flag). Fields following a child
    sequence of the same parent carry late=True."""
    a = text.index('<div class="scheme">')
    b = text.index("</div>", a)
    body = text[a:b]
    seqs, stack, nested, implicit = [], [], False, None
    for m in re.finditer(r'<li class="sequence">\s*Sequence \w+ \([MO]\)(?: \(repetitive\))?<ul>|<li class="field">[^<]*</li>|</ul></li>', body):
        t = m.group(0)
        if t.startswith('<li class="sequence">'):
            s_ = SEQ.match(t)
            if stack:
                nested = True
                stack[-1]["had_child"] = True
            cur = {"name": s_.group(1), "mandatory": s_.group(2) == "M", "repetitive": bool(s_.group(3)), "fields": []}
            seqs.append(cur)
            stack.append(cur)
        elif t.startswith('<li class="field">'):
            f = FIELD.match(t)
            if not f:
                raise ValueError(t)
            if not stack:
                if implicit is None:
                    implicit = {"name": "", "mandatory": True, "repetitive": False, "fields": []}
                    seqs.insert(0, implicit)
                target = implicit
            else:
                target = stack[-1]
            opts = [o for o in f.group(2).split(",") if o]
            fd = {"tag": f.group(1), "options": opts, "mandatory": f.group(3) == "M", "repetitive": bool(f.group(4))}
            if target.get("had_child"):
                fd["late"] = True
            target["fields"].append(fd)
        elif stack:
            stack.pop()
    for sq in seqs:
        sq.pop("had_child", None)
        sq.pop("implicit", None)
    return seqs, nested


def wire_tags(f):
    if not f["options"]:
        return [f["tag"]]
    return [f["tag"] if o == "NONE" else f["tag"] + o for o in f["options"]]


def rules(f, flat, lists):
    """Yield (tag_str, options|None, name_suffix) rules for a scheme field."""
    letters = [o for o in f["options"] if o != "NONE"]
    out = []
    if not f["options"]:
        out.append((f["tag"], None, ""))
    else:
        if "NONE" in f["options"]:
            out.append((f["tag"], None, ""))
        if len(letters) == 1:
            out.append((f["tag"] + letters[0], None, letters[0].lower()))
        elif letters:
            out.append((f["tag"] + "a", letters, "".join(letters).lower()))
    return out


def sample_for(n, seqs, root, anchored, split15):
    """Structure-only sample: mandatory fields, each anchored sequence twice if repetitive."""
    def val(tag):
        return "" if tag.startswith("15") and len(tag) == 3 else "REF" if tag == "20" else "X"
    lines = []
    if split15:
        lines.append(":" + "15" + seqs[0]["fields"][0]["options"][0] + ":")
    for f in root:
        if f["mandatory"]:
            t = wire_tags(f)[0]
            lines.append(f":{t}:{val(t)}")
    for name, anchor, fl, rep in anchored:
        for _ in range(2 if rep else 1):
            lines.append(f":{anchor}:{val(anchor)}")
            for f in (fl[1:] if fl and wire_tags(fl[0])[0] == anchor else fl):
                if f["mandatory"]:
                    t = wire_tags(f)[0]
                    lines.append(f":{t}:{val(t)}")
    return ("{1:F01TESTGB2LAXXX0000000000}{2:I%sTESTGB2LXXXXN}{4:\n" % n) + "\n".join(lines) + "\n-}"


def build(n, text):
    seqs, nested = parse_scheme(text)
    style = set(re.findall(r"SequenceStyle\(Type\.(\w+)", text))
    lists = set(re.findall(r"public List<Field(\w+)> getField", text))
    skip = None
    root, anchored = [], []
    loops = any(q["name"].startswith("Loop") for q in seqs)
    chain_notes = []
    alts = {}
    if n == "204":
        # sequence A and B both start with :20:, so model one repeating group (first = A)
        union = []
        for q in seqs:
            for f in q["fields"]:
                if f["tag"] != "20" and all(wire_tags(f) != wire_tags(g) for g in union):
                    union.append(dict(f, mandatory=False))
        anchored = [("MSG", "20", [{"tag": "20", "options": [], "mandatory": True}] + union, True)]
        chain_notes.append("Sequences A and B both start with :20:; modelled as one repeating group (first occurrence is A, later ones are B); fields optional.")
    elif n == "203":
        flat_fields = seqs[0]["fields"]
        i = next(i for i, f in enumerate(flat_fields) if f["tag"] == "20")
        root = flat_fields[:i]
        anchored = [("LOOP", "20", flat_fields[i:], True)]
    elif nested or loops or (n in {"801"}):
        first = seqs[0]
        chain = list(seqs)
        if first["name"] in ("", "A") and not first["name"].startswith("Loop"):
            root = first["fields"]
            chain = seqs[1:]
            if style == {"SPLIT_BY_15"} or (first["fields"] and first["fields"][0]["tag"] == "15" and len(first["fields"][0]["options"]) == 1):
                marker = first["fields"][0]["options"][0]
                root = first["fields"][1:]
        anchors = {}
        for q in chain:
            f0 = q["fields"][0] if q["fields"] else None
            if f0 is None:
                skip = f"sequence {q['name']} has no fields"
                break
            w = wire_tags(f0)
            anchors[q["name"]] = w
        if not skip:
            all_anchor = [a_ for w in anchors.values() for a_ in w]
            if len(all_anchor) != len(set(all_anchor)):
                skip = "duplicate anchor tags between sequences"
            elif set(all_anchor) & {w for f in root for w in wire_tags(f)}:
                skip = "an anchor tag also appears in the root fields"
        if not skip:
            for q in chain:
                w = anchors[q["name"]]
                body_fields = [f for f in q["fields"][1:] if not f.get("late")] if len(w) == 1 or True else q["fields"]
                late = [f for f in q["fields"] if f.get("late")]
                if late:
                    chain_notes.append(f"{q['name']}: fields after a nested sequence ({', '.join(f['tag'] + ''.join(f['options']) for f in late)}) are not modelled.")
                # drop members that are another sequence's anchor (anchor always wins)
                kept = []
                for f in body_fields:
                    if any(x in all_anchor for x in wire_tags(f)):
                        chain_notes.append(f"{q['name']}: {f['tag']}{''.join(f['options'])} is also another sequence's anchor tag and starts that sequence instead.")
                    else:
                        kept.append(f)
                anchored.append((q["name"], w[0], ([q["fields"][0]] if q["fields"][0]["tag"] != "15" else []) + kept, q["repetitive"]))
                alts[q["name"]] = w[1:]
    elif style <= {"GENERATED_FIXED_WITH_OPTIONAL_TAIL", "CUSTOM", "GENERATED_SLICE"} and len(seqs) > 1:
        a, rest = seqs[0], seqs[1:]
        root = a["fields"]
        if len(a["fields"]) == 0:
            skip = "empty sequence A"
        b = rest[0]
        a_tags = {w for f in a["fields"] for w in wire_tags(f)}
        anchor = wire_tags(b["fields"][0])
        if len(anchor) != 1 or anchor[0] in a_tags:
            skip = f"anchor {anchor} collides with sequence A"
        anchored = [("B", anchor[0], b["fields"], b["repetitive"])]
        tail = rest[1:]
        seen = {w for f in b["fields"] for w in wire_tags(f)} | a_tags
        for t in tail:
            kept = [f for f in t["fields"] if not (set(wire_tags(f)) & seen)]
            anchored[0][2].extend(kept)
    elif style == {"SPLIT_BY_15"}:
        for s in seqs:
            first = s["fields"][0]
            if first["tag"] != "15" or len(first["options"]) != 1:
                skip = "sequence without 15x marker"
                break
        if not skip:
            first_seq = seqs[0]
            root = first_seq["fields"][1:]
            anchored = [(s["name"], "15" + s["fields"][0]["options"][0], s["fields"][1:], s["repetitive"]) for s in seqs[1:]]
    elif len(seqs) == 1 and style <= {"GENERATED_FIXED_WITH_OPTIONAL_TAIL"}:
        root = seqs[0]["fields"]
    else:
        skip = f"unsupported style {sorted(style)}"
    if skip:
        return None, skip, seqs
    flat = not anchored
    low = f"mt{n}"
    title = re.search(r"MT %s - ([^.\n]+)" % n, text)
    title = title.group(1).strip() if title else ""
    L = ["---",
         f"# MT{n} ({title}) — starter schema generated by scripts/gen-schemas-from-prowide.py from the",
         f"# Prowide Core message structure (prowide/prowide-core @ {COMMIT[:12]}, Apache-2.0).",
         "# Structure only (tags, options, mandatory/optional); formats are captured verbatim and",
         "# decoded downstream. Not a licensed SRU import; network validation rules not reproduced."]
    if anchored:
        L.append("# Repeating/later sequences have no :16R:/:16S: wrapper: anchored sequences (ADR-0013).")
    L += ["field_types:", f"- name: {low}_text", "  pattern:", "  - kind: rest", "    name: value", "",
          "messages:", f"- message: MT{n}", f"  category: {CAT.get(n[0], 'other')}", "  version: starter",
          "  coverage:", f"    source: Prowide Core structure (SRU 2025) via prowide-core {COMMIT[:12]}; not a licensed SRU import.",
          "    exact: false", f"    expected_sequences: {len(anchored)}",
          f"    expected_fields: {len(root) + sum(len(a[2]) for a in anchored)}", "    notes:",
          '    - "Fields captured verbatim; conditional (C/D/E) network rules not reproduced."']
    if style and anchored and "SPLIT_BY_15" not in style:
        L.append('    - "Optional tail sequence fields that reuse a tag of an earlier sequence are not modelled."')
    for note in chain_notes:
        L.insert(L.index("    notes:") + 1, f'    - "{note}"')
    if anchored:
        L.append("  sequences:")
        for name, anchor, fl, rep in anchored:
            members = []
            for f in fl:
                for w in wire_tags(f):
                    if w != anchor and w not in members:
                        members.append(w)
            L += [f"    {name}:", f'      anchor_tag: "{anchor}"']
            if alts.get(name):
                L.append("      alt_anchor_tags: [" + ", ".join(f'"{x}"' for x in alts[name]) + "]")
            L += ["      member_tags: [" + ", ".join(f'"{m}"' for m in members) + "]", "      repeat: true"]
    L.append("  fields:")
    used = set()

    def emit(path, entity, fl, prefix, flat, skip_tag=None):
        for f in fl:
            for tag, opts, suf in rules(f, flat, lists):
                if tag == skip_tag:
                    continue
                if opts:
                    nm = NAMES.get(f["tag"], f"f{f['tag']}") + "_" + suf
                else:
                    nm = NAMES.get(tag) or (NAMES.get(f["tag"]) if tag == f["tag"] else None) or f"f{tag}"
                nm = prefix + nm
                k = 2
                orig = nm
                while (path, nm) in used:
                    nm = f"{orig}_{k}"
                    k += 1
                used.add((path, nm))
                r = [f"  - path: {path}", (f"    tag: {tag}" if opts else f'    tag: "{tag}"')]
                if opts:
                    r.append("    options: [" + ", ".join(f'"{o}"' for o in opts) + "]")
                r += [f"    name: {nm}", f"    type: {low}_text"]
                if f["mandatory"] and not (f["options"] and len(rules(f, flat, lists)) > 1):
                    r += ["    required: true", "    min: 1"]
                letters = f["options"] and [o for o in f["options"] if o != "NONE"]
                listy = f.get('repetitive') or any(f"{f['tag']}{o}" in lists for o in (letters or [""]))
                if flat and not listy:
                    r.append("    max: 1")
                r += [f"    entity: {entity}", f"    column: {nm}"]
                if opts:
                    r += ["    render:", f"      option: {opts[0]}"]
                L.append("\n".join(r))

    emit("$", f"{low}_message", root, "", flat)
    for name, anchor, fl, rep in anchored:
        emit(name, f"{low}_{name.lower()}", fl, name.lower() + "_", False, skip_tag=None)
    return "\n".join(L) + "\n", None, (seqs, sample_for(n, seqs, root, anchored, style == {"SPLIT_BY_15"}))


def main():
    samples_dir = None
    if "--samples" in sys.argv:
        samples_dir = sys.argv[sys.argv.index("--samples") + 1]
    args = [a for a in sys.argv[1:] if not a.startswith("--") and a != samples_dir]
    only = None
    if "--only" in sys.argv:
        only = set(sys.argv[sys.argv.index("--only") + 1].split(","))
        args = [a for a in args if a not in only and "," not in a]
    src_dir, out_dir, struct_path = args[:3]
    structures, skipped, written = {}, {}, []
    for fn in sorted(os.listdir(src_dir)):
        m = re.fullmatch(r"MT(\d{3})\.java", fn)
        if not m:
            continue
        n = m.group(1)
        text = open(os.path.join(src_dir, fn), encoding="utf-8").read()
        if '<div class="scheme">' not in text:
            continue
        title = re.search(r"MT %s - ([^.\n]+)" % n, text)
        seqs, nested = parse_scheme(text)
        structures[f"MT{n}"] = {"title": title.group(1).strip() if title else "",
                                "source": SRC.format(c=n[0], n=n), "commit": COMMIT, "sequences": seqs}
        if only is not None and f"MT{n}" not in only:
            continue
        if n.startswith("0"):
            skipped[f"MT{n}"] = "service/system message"
            continue
        path = os.path.join(out_dir, f"mt{n}.yaml")
        if os.path.exists(path):
            continue
        yaml, why, extra = build(n, text)
        if why:
            skipped[f"MT{n}"] = why
            continue
        open(path, "w").write(yaml)
        if samples_dir:
            os.makedirs(samples_dir, exist_ok=True)
            open(os.path.join(samples_dir, f"generated_MT{n}.fin"), "w").write(extra[1])
        written.append(f"MT{n}")
    json.dump(structures, open(struct_path, "w"), indent=1)
    print("written", len(written), " ".join(written))
    print("skipped", json.dumps(skipped))


if __name__ == "__main__":
    main()
