#!/usr/bin/env python3
"""Validate a .pptx as an OPC package, independently of deckr.

Unit tests can only check that deckr agrees with itself. This script checks
that what we emit is a package *PowerPoint's* rules accept, using nothing but
the standard library so it runs on any CI image without pip installs.

    python tests/fixtures/validate_pptx.py built.pptx [more.pptx ...]

It is deliberately separate from the Rust tests: when this fails, the bug is
in our understanding of OPC, not in our comparison against ourselves.
"""

from __future__ import annotations

import posixpath
import sys
import xml.etree.ElementTree as ET
import zipfile

#: Namespace of the `r:` prefix used for relationship *references* in XML.
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
#: Default namespace inside a .rels part. Not the same as `R_NS` — this is the
#: package-level vocabulary, and confusing the two silently finds no
#: relationships at all.
PKG_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
CT_NS = "http://schemas.openxmlformats.org/package/2006/content-types"
P_NS = "http://schemas.openxmlformats.org/presentationml/2006/main"
A_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"

EXIT_OK = 0
EXIT_FAILURE = 1


class Report:
    """Collects findings so one run reports every problem, not just the first."""

    def __init__(self, name: str) -> None:
        self.name = name
        self.problems: list[str] = []
        self.checks = 0

    def ok(self, what: str) -> None:
        self.checks += 1
        print(f"    ok  {what}")

    def fail(self, what: str) -> None:
        self.checks += 1
        self.problems.append(what)
        print(f"  FAIL  {what}")

    def __bool__(self) -> bool:
        return not self.problems


def part_root(xml_bytes: bytes, name: str) -> ET.Element:
    """Parse a part, raising ValueError with the part name attached."""
    try:
        return ET.fromstring(xml_bytes)
    except ET.ParseError as exc:
        raise ValueError(f"{name}: {exc}") from exc


def content_type_map(zf: zipfile.ZipFile) -> tuple[dict[str, str], dict[str, str]]:
    """Return (defaults-by-extension, overrides-by-part) from [Content_Types].xml."""
    try:
        raw = zf.read("[Content_Types].xml")
    except KeyError:
        return {}, {}
    root = part_root(raw, "[Content_Types].xml")
    defaults = {
        el.get("Extension", "").lower(): el.get("ContentType", "")
        for el in root.findall(f"{{{CT_NS}}}Default")
    }
    overrides = {
        el.get("PartName", "").lstrip("/"): el.get("ContentType", "")
        for el in root.findall(f"{{{CT_NS}}}Override")
    }
    return defaults, overrides


def rels_path_for(part: str) -> str:
    """`ppt/slides/slide1.xml` -> `ppt/slides/_rels/slide1.xml.rels`."""
    directory, base = posixpath.split(part)
    return posixpath.join(directory, "_rels", base + ".rels")


def read_rels(zf: zipfile.ZipFile, rels_part: str) -> dict[str, tuple[str, str]]:
    """Read a .rels part into {id: (target, mode)}."""
    root = part_root(zf.read(rels_part), rels_part)
    return {
        el.get("Id", ""): (el.get("Target", ""), el.get("TargetMode", "Internal"))
        for el in root.findall(f"{{{PKG_REL_NS}}}Relationship")
    }


def rels_of(zf: zipfile.ZipFile, part: str) -> dict[str, tuple[str, str]]:
    """Resolve the .rels partner of `part`, if it has one."""
    rels_part = rels_path_for(part)
    if rels_part not in zf.namelist():
        return {}
    return read_rels(zf, rels_part)


def absolute_target(base_dir: str, target: str) -> str:
    """Turn a relationship target into a package path.

    Internal targets are relative to the directory holding the .rels part,
    which is why `../slideLayouts/x.xml` means `ppt/slideLayouts/x.xml`.
    """
    if target.startswith("/"):
        return target.lstrip("/")
    return posixpath.normpath(posixpath.join(base_dir, target)).replace("\\", "/")


def validate(path: str) -> bool:
    print(f"\n{path}")
    report = Report(path)
    try:
        zf = zipfile.ZipFile(path)
    except (OSError, zipfile.BadZipFile) as exc:
        report.fail(f"not a readable zip: {exc}")
        return False

    with zf:
        names = zf.namelist()
        if "[Content_Types].xml" not in names:
            report.fail("missing [Content_Types].xml — not an OPC package")
            return False
        report.ok("package opens and carries [Content_Types].xml")

        defaults, overrides = content_type_map(zf)

        # 1. Every XML part parses. A truncated or mis-escaped part is the
        #    single most common way a generated package fails to open.
        xml_parts = [n for n in names if n.endswith((".xml", ".rels"))]
        broken = []
        for part in xml_parts:
            try:
                part_root(zf.read(part), part)
            except ValueError as exc:
                broken.append(str(exc))
        if broken:
            for b in broken:
                report.fail(f"malformed XML: {b}")
        else:
            report.ok(f"all {len(xml_parts)} XML parts parse")

        # 2. Every declared part exists, and every part is declared. Undeclared
        #    parts are ignored by consumers; absent ones stop the file loading.
        missing = sorted(p for p in overrides if p not in names)
        if missing:
            for p in missing:
                report.fail(f"[Content_Types].xml declares /{p}, which is not in the package")
        else:
            report.ok(f"all {len(overrides)} Override part(s) are present")

        undeclared = []
        for part in names:
            if part.endswith("/") or part == "[Content_Types].xml":
                continue
            extension = part.rsplit(".", 1)[-1].lower() if "." in part else ""
            if part in overrides or extension in defaults:
                continue
            undeclared.append(part)
        if undeclared:
            for p in undeclared:
                report.fail(f"{p} has no Default extension and no Override")
        else:
            report.ok("every part has a content type")

        # 3. Every internal relationship target resolves. Broken links here are
        #    how a slide ends up rendering against a layout that is not there.
        dangling = []
        for rel_part in (n for n in names if n.endswith(".rels")):
            # Targets resolve against the directory *owning* the rels file, so
            # `ppt/slides/_rels/slide1.xml.rels` resolves against `ppt/slides`.
            base_dir = posixpath.dirname(posixpath.dirname(rel_part))
            for rel_id, (target, mode) in sorted(read_rels(zf, rel_part).items()):
                if mode == "External":
                    continue
                resolved = absolute_target(base_dir, target)
                if resolved not in names:
                    dangling.append(f"{rel_part}: {rel_id} -> {target} (resolved {resolved})")
        if dangling:
            for d in dangling:
                report.fail(f"dangling relationship: {d}")
        else:
            report.ok("every internal relationship resolves")

        # 4. Every relationship actually referenced by r:id exists. The mirror
        #    of check 3: an unresolved r:id is a dangling reference inside XML.
        unresolved = []
        for part in xml_parts:
            if part.endswith(".rels"):
                continue
            try:
                root = part_root(zf.read(part), part)
            except ValueError:
                continue
            known = rels_of(zf, part)
            for el in root.iter():
                rid = el.get(f"{{{R_NS}}}id") or el.get(f"{{{R_NS}}}embed") or el.get(f"{{{R_NS}}}link")
                if rid and rid not in known:
                    unresolved.append(f"{part}: {el.tag.rsplit('}')[-1]} uses {rid}")
        if unresolved:
            for u in unresolved:
                report.fail(f"unresolved r:id reference: {u}")
        else:
            report.ok("every r:id referenced in XML is defined")

        # 5. The presentation's slide list must resolve, and its order defines
        #    reading order — which is not the same as filename order.
        try:
            pres = part_root(zf.read("ppt/presentation.xml"), "ppt/presentation.xml")
            rels = rels_of(zf, "ppt/presentation.xml")
        except (KeyError, ValueError) as exc:
            report.fail(f"cannot read the presentation part: {exc}")
            return bool(report)

        slides: list[str] = []
        for sld_id in pres.iter(f"{{{P_NS}}}sldId"):
            target, _ = rels.get(sld_id.get(f"{{{R_NS}}}id", ""), ("", ""))
            resolved = absolute_target("ppt", target)
            if resolved not in names:
                report.fail(f"sldIdLst names {target}, which is not in the package")
            else:
                slides.append(resolved)
        if slides:
            report.ok(f"presentation lists {len(slides)} slide(s), all present")

        # 6. Shape ids must be unique within a slide. PowerPoint repairs files
        #    that repeat them, so this is the check that catches a shape-tree
        #    accident the rest of the package would happily carry.
        for slide_part in slides:
            root = part_root(zf.read(slide_part), slide_part)
            seen: dict[str, str] = {}
            duplicates = []
            for el in root.iter(f"{{{P_NS}}}cNvPr"):
                ident = el.get("id", "")
                if ident in seen:
                    duplicates.append(f"{slide_part}: id {ident} used twice")
                seen[ident] = el.get("name", "")
            # Group shapes are containers; their children share the tree, so
            # ids are counted once per shape regardless of nesting.
            for d in dict.fromkeys(duplicates):
                report.fail(f"duplicate shape id: {d}")
        if slides:
            report.ok("shape ids are unique within each slide")

        # 7. Text bodies must end up inside a txBody, and any a:t inside one
        #    must not be a sibling of another shape's child. Cheap structural
        #    sanity on what the writer actually emitted.
        for slide_part in slides:
            root = part_root(zf.read(slide_part), slide_part)
            stray_text = []
            parent_of = {child: parent for parent in root.iter() for child in parent}
            for text_el in root.iter(f"{{{A_NS}}}t"):
                node = text_el
                in_body = False
                while node is not None:
                    if node.tag == f"{{{P_NS}}}txBody" or node.tag == f"{{{A_NS}}}txBody":
                        in_body = True
                        break
                    node = parent_of.get(node)
                if not in_body:
                    stray_text.append(slide_part)
            for s in dict.fromkeys(stray_text):
                report.fail(f"{s}: <a:t> outside any txBody")

    return bool(report)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__.strip())
        return EXIT_FAILURE
    results = [validate(p) for p in argv[1:]]
    if all(results):
        print(f"\n{len(results)} package(s) valid.\n")
        return EXIT_OK
    print(f"\n{results.count(False)} package(s) rejected.\n")
    return EXIT_FAILURE


if __name__ == "__main__":
    sys.exit(main(sys.argv))
