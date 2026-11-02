import argparse
import collections
import datetime
import hashlib
import json
import math
import pathlib
import re
import struct


def save_json(destination, value):
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def analyze(sample, expected_hash):
    image = sample.read_bytes()
    actual_hash = hashlib.sha256(image).hexdigest()
    if actual_hash != expected_hash:
        raise ValueError("Sample SHA256 does not match the accepted package")

    def read_bytes(offset, count):
        if offset < 0 or count < 0 or offset + count > len(image):
            raise ValueError(f"File range outside sample: {offset:#x}+{count:#x}")
        return image[offset:offset + count]

    def unpack_fields(format_string, offset):
        return struct.unpack(format_string, read_bytes(offset, struct.calcsize(format_string)))

    def read_string(offset, limit=512):
        available = read_bytes(offset, min(limit, len(image) - offset))
        terminator = available.find(b"\x00")
        if terminator < 0:
            raise ValueError(f"Unterminated PE string at {offset:#x}")
        return available[:terminator].decode("ascii", errors="backslashreplace")

    if read_bytes(0, 2) != b"MZ":
        raise ValueError("DOS signature is not MZ")
    pe_offset = unpack_fields("<I", 0x3C)[0]
    if read_bytes(pe_offset, 4) != b"PE\x00\x00":
        raise ValueError("PE signature is missing")
    machine, section_count, timestamp, symbol_pointer, symbol_count, optional_size, characteristics = unpack_fields("<HHIIIHH", pe_offset + 4)
    if not 1 <= section_count <= 96:
        raise ValueError("Unexpected PE section count")
    optional_offset = pe_offset + 24
    read_bytes(optional_offset, optional_size)
    optional_magic = unpack_fields("<H", optional_offset)[0]
    if optional_magic == 0x10B:
        pe_kind, pointer_size, directory_base, count_offset = "PE32", 4, 96, 92
        image_base = unpack_fields("<I", optional_offset + 28)[0]
    elif optional_magic == 0x20B:
        pe_kind, pointer_size, directory_base, count_offset = "PE32+", 8, 112, 108
        image_base = unpack_fields("<Q", optional_offset + 24)[0]
    else:
        raise ValueError(f"Unsupported optional header magic: {optional_magic:#x}")
    if optional_size < directory_base:
        raise ValueError("Optional header is shorter than its data-directory prefix")
    entry_rva = unpack_fields("<I", optional_offset + 16)[0]
    header_size = unpack_fields("<I", optional_offset + 60)[0]
    directory_count = unpack_fields("<I", optional_offset + count_offset)[0]
    available_directory_count = (optional_size - directory_base) // 8
    if directory_count > available_directory_count:
        raise ValueError("Data directories exceed the optional header")
    directories = []
    for directory_index in range(min(directory_count, 16)):
        address, size = unpack_fields("<II", optional_offset + directory_base + directory_index * 8)
        directories.append({"index": directory_index, "address": address, "size": size, "addressKind": "fileOffset" if directory_index == 4 else "rva"})
    section_table = optional_offset + optional_size
    sections = []
    for section_index in range(section_count):
        record_offset = section_table + section_index * 40
        record = read_bytes(record_offset, 40)
        name = record[:8].rstrip(b"\x00").decode("ascii", errors="backslashreplace")
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from("<IIII", record, 8)
        section_flags = struct.unpack_from("<I", record, 36)[0]
        raw_data = read_bytes(raw_offset, raw_size) if raw_size else b""
        counts = collections.Counter(raw_data)
        entropy = -sum((count / len(raw_data)) * math.log2(count / len(raw_data)) for count in counts.values()) if raw_data else 0.0
        sections.append({"name": name, "headerFileOffset": record_offset, "virtualAddress": virtual_address, "virtualSize": virtual_size, "rawOffset": raw_offset, "rawSize": raw_size, "characteristics": f"0x{section_flags:08x}", "executable": bool(section_flags & 0x20000000), "writable": bool(section_flags & 0x80000000), "entropyBitsPerByte": round(entropy, 4)})

    def rva_to_file(rva, count=1):
        if rva < header_size:
            read_bytes(rva, count)
            return rva
        for section in sections:
            relative = rva - section["virtualAddress"]
            if 0 <= relative < max(section["virtualSize"], section["rawSize"]):
                if relative + count > section["rawSize"]:
                    raise ValueError(f"RVA {rva:#x} is not backed by section bytes")
                offset = section["rawOffset"] + relative
                read_bytes(offset, count)
                return offset
        raise ValueError(f"Unmapped RVA: {rva:#x}")

    import_directory = directories[1] if len(directories) > 1 else {"address": 0, "size": 0}
    modules = []
    if not import_directory["address"] or import_directory["size"] < 20:
        raise ValueError("Sample has no readable import descriptor directory")
    descriptor_end_found = False
    for descriptor_index in range(min(import_directory["size"] // 20, 2048)):
        descriptor_rva = import_directory["address"] + descriptor_index * 20
        descriptor_offset = rva_to_file(descriptor_rva, 20)
        lookup_rva, import_timestamp, forwarder, name_rva, first_thunk = unpack_fields("<IIIII", descriptor_offset)
        if not any((lookup_rva, import_timestamp, forwarder, name_rva, first_thunk)):
            descriptor_end_found = True
            break
        module_name = read_string(rva_to_file(name_rva))
        thunk_start = lookup_rva or first_thunk
        if not thunk_start:
            raise ValueError("Import descriptor lacks both lookup table and IAT")
        functions = []
        thunk_end_found = False
        ordinal_mask = 1 << (pointer_size * 8 - 1)
        for thunk_index in range(65536):
            thunk_rva = thunk_start + thunk_index * pointer_size
            thunk_value = unpack_fields("<I" if pointer_size == 4 else "<Q", rva_to_file(thunk_rva, pointer_size))[0]
            if thunk_value == 0:
                thunk_end_found = True
                break
            function = {"lookupRva": thunk_rva, "iatRva": first_thunk + thunk_index * pointer_size}
            if thunk_value & ordinal_mask:
                function.update({"name": None, "ordinal": thunk_value & 0xFFFF})
            else:
                name_offset = rva_to_file(thunk_value, 3)
                function.update({"name": read_string(name_offset + 2), "hint": unpack_fields("<H", name_offset)[0], "nameRva": thunk_value, "nameFileOffset": name_offset + 2})
            functions.append(function)
        if not thunk_end_found:
            raise ValueError("Import thunk table lacks a bounded terminator")
        modules.append({"name": module_name, "descriptorRva": descriptor_rva, "descriptorFileOffset": descriptor_offset, "lookupUsesIatFallback": not bool(lookup_rva), "functions": functions})
    if not descriptor_end_found or not modules:
        raise ValueError("Import descriptor table lacks readable modules or a bounded terminator")

    dynamic_loading = [{"module": module["name"], **function} for module in modules for function in module["functions"] if function.get("name", "") and re.match(r"(?:LoadLibrary|GetProcAddress)", function["name"])]
    overlay_offset = max([header_size] + [section["rawOffset"] + section["rawSize"] for section in sections if section["rawSize"]])
    signature_offset = image.find(b"\xef\xbe\xad\xdeNullsoftInst", overlay_offset)
    nsis_header = None
    if signature_offset >= overlay_offset + 4:
        first_header_offset = signature_offset - 4
        words = unpack_fields("<7I", first_header_offset)
        nsis_header = {"fileOffset": first_header_offset, "signatureOffset": signature_offset, "signatureHex": read_bytes(signature_offset, 16).hex(), "flags": f"0x{words[0]:08x}", "lengthOfHeader": words[5], "lengthOfAllFollowingData": words[6], "availableBytesFromHeader": len(image) - first_header_offset, "firstHeaderHex": read_bytes(first_header_offset, 28).hex(), "layoutBasis": "NSIS Source/exehead/fileform.h firstheader; observation only, compressed payload not decoded"}
    security_directory = directories[4] if len(directories) > 4 else {"address": 0, "size": 0}
    if security_directory["address"] and security_directory["size"]:
        read_bytes(security_directory["address"], security_directory["size"])
    selected_strings = []
    string_pattern = re.compile(r"Nullsoft|NSIS|Uninstall|XY.Switch|GetProcAddress|LoadLibrary|kernel32\.dll", re.IGNORECASE)
    for match in re.finditer(rb"[\x20-\x7e]{6,160}", image):
        text = match.group().decode("ascii")
        if string_pattern.search(text):
            selected_strings.append({"encoding": "ascii", "fileOffset": match.start(), "text": text})
        if len(selected_strings) >= 80:
            break
    identity = {"fileName": sample.name, "bytes": len(image), "md5": hashlib.md5(image).hexdigest(), "sha256": actual_hash, "dosSignature": "MZ", "peSignatureHex": read_bytes(pe_offset, 4).hex(), "peHeaderOffset": pe_offset, "machine": f"0x{machine:04x}", "machineName": {0x14C: "I386", 0x8664: "AMD64", 0xAA64: "ARM64"}.get(machine, "unknown"), "kind": pe_kind, "optionalHeaderMagic": f"0x{optional_magic:04x}", "entryPointRva": entry_rva, "entryPointFileOffset": rva_to_file(entry_rva), "imageBase": image_base, "sectionCount": section_count, "coffCharacteristics": f"0x{characteristics:04x}", "coffTimestampRaw": timestamp, "subsystem": unpack_fields("<H", optional_offset + 68)[0]}
    return {"identity": identity, "sections": sections, "directories": directories, "imports": {"quality": "parsed-bounded-descriptors-and-thunks", "moduleCount": len(modules), "functionCount": sum(len(module["functions"]) for module in modules), "directory": import_directory, "modules": modules, "dynamicLoadingAnchors": dynamic_loading, "claimBoundary": "Direct imports are static references. They do not establish complete runtime capabilities or absence of networking."}, "overlay": {"fileOffset": overlay_offset, "bytes": len(image) - overlay_offset, "sha256": hashlib.sha256(image[overlay_offset:]).hexdigest(), "nsisFirstHeader": nsis_header}, "certificateDirectory": {**security_directory, "present": bool(security_directory["address"] and security_directory["size"]), "signatureVerification": "not-performed"}, "selectedStrings": selected_strings}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--business-input", type=pathlib.Path, required=True)
    parser.add_argument("--repo-root", type=pathlib.Path, required=True)
    parser.add_argument("--case-root", type=pathlib.Path, required=True)
    arguments = parser.parse_args()
    case_root = arguments.case_root.resolve()
    scope = (case_root / "scope.md").read_text(encoding="utf-8-sig")
    for required in ("- status: granted", "- basis: own_system", "- mode: offline", "- ready_for_act: true"):
        if required not in scope:
            raise ValueError(f"Scope gate missing: {required}")
    input_data = json.loads(arguments.business_input.read_text(encoding="utf-8-sig"))
    packages = [package for package in input_data["packages"] if package["format"] == "nsis"]
    if len(packages) != 1:
        raise ValueError("Acceptance input must identify exactly one NSIS sample")
    package = packages[0]
    sample = pathlib.Path(package["path"]).resolve()
    if str(sample) not in scope:
        raise ValueError("Sample is not in the granted scope")
    manifest_path = (arguments.repo_root.resolve() / package["manifest"]).resolve()
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    manifest_file = next(item for item in manifest["files"] if item["name"] == sample.name)
    if any(manifest_file[field] != package[field] for field in ("format", "bytes", "sha256")):
        raise ValueError("Package input differs from the build manifest")
    analysis = analyze(sample, package["sha256"])
    if analysis["identity"]["bytes"] != package["bytes"]:
        raise ValueError("Package length differs from the build manifest")
    analysis.update({"schemaVersion": 1, "observedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(), "tool": "Python standard library hashlib/struct/re; sample read only", "sourceCommit": input_data["sourceCommit"], "sourceRun": input_data["sourceRun"], "chargedApiCalls": 0, "sampleExecuted": False})
    evidence_root = case_root / "evidence"
    save_json(evidence_root / "pe-analysis.json", analysis)
    save_json(evidence_root / "imports.json", analysis["imports"])
    save_json(evidence_root / "selected-strings.json", {"sampleSha256": package["sha256"], "claimBoundary": "Selected static strings only; no runtime behavior or whole payload claims", "strings": analysis["selectedStrings"]})
    save_json(evidence_root / "manifest-binding.json", {"sourceCommit": input_data["sourceCommit"], "sourceRun": input_data["sourceRun"], "manifestPath": package["manifest"], "manifestSha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(), "manifestEntry": manifest_file, "sampleSha256Before": analysis["identity"]["sha256"], "sampleSha256After": hashlib.sha256(sample.read_bytes()).hexdigest(), "matched": True})
    print(json.dumps({"passed": True, "identity": analysis["identity"], "moduleCount": analysis["imports"]["moduleCount"], "functionCount": analysis["imports"]["functionCount"], "moduleNames": [module["name"] for module in analysis["imports"]["modules"]], "dynamicLoadingNames": [function["name"] for function in analysis["imports"]["dynamicLoadingAnchors"]], "overlay": analysis["overlay"], "certificateDirectory": analysis["certificateDirectory"], "selectedStringCount": len(analysis["selectedStrings"]), "sampleExecuted": False, "chargedApiCalls": 0}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()

