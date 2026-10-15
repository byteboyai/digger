"""Fetch two public-domain novels from Project Gutenberg and turn them into Markdown books.
Not committed: data/ is ignored. Run: python3 -I prepare.py
  hlm  (紅樓夢, Gutenberg #24264): hard-wrapped lines, paragraphs start with two ideographic spaces (U+3000)
  pp   (Pride and Prejudice, #1342): hard-wrapped, blank-line paragraphs
Output per book: data/<name>.md (whole book), data/<name>/NNN.md (one file per chapter), in Markdown with '# ' headings."""
import os, re, sys, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "data")
os.makedirs(D, exist_ok=True)

def fetch(num):
    p = os.path.join(D, f"pg{num}.txt")
    if not os.path.exists(p):
        with urllib.request.urlopen(f"https://www.gutenberg.org/cache/epub/{num}/pg{num}.txt", timeout=60) as r:
            open(p, "wb").write(r.read())
    t = open(p, encoding="utf-8").read().replace("\r\n", "\n")
    return t[t.index("\n", t.index("*** START")) : t.index("*** END")]

def hlm():
    body = fetch(24264)
    lines = body.split("\n")
    chapters, cur, i = [], None, 0
    head = re.compile(r"^(第[〇一二三四五六七八九十百零]+回)(?:[\u3000 ]+(.*))?$")
    while i < len(lines):
        line = lines[i].strip()
        m = head.match(line)
        if m:
            rest = (m.group(2) or "").strip()
            tail = ""
            if not rest:  # "第三回" alone: the title is on the next non-empty line
                i += 1
                while lines[i].strip() == "":
                    i += 1
                rest = lines[i].strip()
            if len(rest) > 36 and re.search(r"[\u3000 ]+\S", rest[16:]):  # heading and first words on one line
                k = rest.rfind(" ", 0, 40) if " " in rest[:40] else -1
                if k > 0:
                    rest, tail = rest[:k].strip(), rest[k:].strip()
            cur = [m.group(1) + "　" + rest, [("\u3000\u3000" + tail) if tail else None]]
            chapters.append(cur)
            i += 1
            continue
        if cur is not None:
            cur[1].append(lines[i])
        i += 1
    out = []
    for title, raw in chapters:
        paras, buf = [], ""
        for l in raw:
            if l is None:
                continue
            if l.startswith("\u3000\u3000"):
                if buf.strip():
                    paras.append(buf)
                buf = l
            elif l.strip():
                buf += l.strip() if buf else l
        if buf.strip():
            paras.append(buf)
        out.append((title, paras))
    return out

def pp():
    body = fetch(1342)
    parts = re.split(r"\n\s*\n(?=(?i:chapter) [IVXLC]+\.?\]?\s*\n)", body)
    out = []
    for p in parts:
        m = re.match(r"^\s*((?i:chapter) [IVXLC]+)\.?\]?\s*\n(.*)$", p, re.S)
        if not m:
            continue
        paras = [re.sub(r"\s*\n\s*", " ", q).strip() for q in re.split(r"\n\s*\n", m.group(2))]
        out.append((m.group(1), [q for q in paras if q]))
    return out

def write(name, chapters):
    os.makedirs(os.path.join(D, name), exist_ok=True)
    whole = []
    for i, (title, paras) in enumerate(chapters, 1):
        md = f"# {title}\n\n" + "\n\n".join(paras) + "\n"
        open(os.path.join(D, name, f"{i:03d}.md"), "w", encoding="utf-8").write(md)
        whole.append(md)
    open(os.path.join(D, f"{name}.md"), "w", encoding="utf-8").write("\n".join(whole))
    n = sum(len(p) for _, p in chapters)
    chars = sum(len(x) for _, p in chapters for x in p)
    print(f"{name}: {len(chapters)} chapters, {n} paragraphs, {chars} characters")

if __name__ == "__main__":
    write("hlm", hlm())
    write("pp", pp())
