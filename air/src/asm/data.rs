//! Named data segments: placing unplaced segments inside a declared `;; @data`
//! region and deriving each segment's `$name.ptr` / `$name.len` globals.

use wasmtime::Result;

use crate::fail;

use super::scan::{DataSegment, ModuleScan, scan_module, token_len};
use super::source::{DataRegion, Expanded};

/// A literal address: decimal, or hexadecimal with an `0x` prefix.
fn address(text: &str, described: &str) -> Result<u32> {
    let token = text.trim().replace('_', "");
    let parsed = match token.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => token.parse(),
    };
    parsed.map_err(|_| wasmtime::Error::msg(format!("`{token}` at {described} is not an address")))
}

/// Record the `;; @data <start>[..<end>]` region. Declaring it is what keeps
/// the harness from guessing at addresses the author is already using for
/// scratch space, buffers or a lib's ABI map.
pub(super) fn set_data_region(
    args: &str,
    file: &std::path::Path,
    line: usize,
    expanded: &mut Expanded,
) -> Result<()> {
    if let Some(first) = &expanded.region {
        return fail(format!(
            "`{}:{line}` declares a second `@data` region; `{}:{} already declared one",
            file.display(),
            first.file.display(),
            first.line
        ));
    }
    let described = format!("{}:{line}", file.display());
    let (start_text, end_text) = match args.trim().split_once("..") {
        Some((start, end)) => (start, Some(end)),
        None => (args.trim(), None),
    };
    let start = address(start_text, &described)?;
    let end = end_text.map(|text| address(text, &described)).transpose()?;
    if let Some(end) = end {
        if end <= start {
            return fail(format!(
                "`@data` region at {described} ends at {end:#x}, \
                 which is not above its start {start:#x}"
            ));
        }
    }
    expanded.region = Some(DataRegion {
        start,
        end,
        file: file.to_path_buf(),
        line,
    });
    Ok(())
}

/// True when a data form places itself. Only the text before the first string
/// literal counts, so `i32.const` inside the data itself is not an offset.
fn has_offset(form: &str) -> bool {
    form.split('"')
        .next()
        .is_some_and(|head| head.contains("i32.const"))
}

/// Give every named segment that did not place itself an address inside the
/// `@data` region, packed in source order.
///
/// This is the other hand-maintained number in a WAT source. `.len` already
/// comes from the harness; without this the author still chains addresses by
/// hand, so inserting a word into one string moves every string after it.
/// Segments that state an offset keep it: the memory map stays author-owned,
/// and the region is the part handed over.
pub(super) fn place_data_segments(expanded: &mut Expanded) -> Result<()> {
    let scan = scan_module(&expanded.text);
    // (byte to insert at, text to insert), applied last-first so earlier
    // offsets stay valid.
    let mut edits: Vec<(usize, String)> = Vec::new();
    for module in &scan.modules {
        let mut taken: Vec<(&str, u32, u32)> = Vec::new();
        let mut unplaced = Vec::new();
        for segment in &module.data {
            let form = &expanded.text[segment.start..=segment.end];
            let (file, line) = expanded.origin(segment.line);
            let described = format!("{} at {}:{line}", segment.name, file.display());
            let length = data_length(form, &described)?;
            if has_offset(form) {
                taken.push((&segment.name, data_address(form, &described)?, length));
            } else {
                unplaced.push((segment, length, described));
            }
        }
        if unplaced.is_empty() {
            continue;
        }
        let Some(region) = &expanded.region else {
            let (_, _, described) = &unplaced[0];
            return fail(format!(
                "data segment `{described}` has no offset and no `;; @data <start>` \
                 region is declared; add the directive or give the segment a literal offset"
            ));
        };
        let mut next = region.start;
        for (segment, length, described) in unplaced {
            if let Some(end) = region.end {
                if next + length > end {
                    return fail(format!(
                        "data segment `{described}` does not fit the `@data` region: \
                         {next:#x}..{:#x} passes its end {end:#x}",
                        next + length
                    ));
                }
            }
            for (other, address, other_length) in &taken {
                if next < address + other_length && *address < next + length {
                    return fail(format!(
                        "the `@data` region would place `{described}` at \
                         {next:#x}..{:#x}, over `{other}` at {address:#x}..{:#x}",
                        next + length,
                        address + other_length
                    ));
                }
            }
            edits.push((
                identifier_end(&expanded.text, segment),
                format!(" (i32.const {next:#x})"),
            ));
            next += length;
        }
    }
    edits.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, text) in edits {
        expanded.text.insert_str(at, &text);
    }
    Ok(())
}

/// The byte just past a segment's `$name`, where its offset belongs.
fn identifier_end(text: &str, segment: &DataSegment) -> usize {
    let bytes = text.as_bytes();
    let start = segment.start
        + bytes[segment.start..=segment.end]
            .iter()
            .position(|b| *b == b'$')
            .expect("a named segment has an identifier");
    start + token_len(&bytes[start..])
}

/// Named data segments are the one place authored WAT carries a hand-maintained
/// byte count, and a stale count truncates output without ever failing
/// validation. For every `(data $name (i32.const <addr>) "...")` the harness
/// appends `$name.ptr` and `$name.len` globals, so the author reads the length
/// instead of restating it. Unnamed segments are untouched.
pub(super) fn append_data_globals(expanded: &mut Expanded) -> Result<()> {
    let scan = scan_module(&expanded.text);
    // Insert from the last module backwards so earlier byte offsets stay valid.
    let mut modules: Vec<&ModuleScan> = scan
        .modules
        .iter()
        .filter(|module| !module.data.is_empty())
        .collect();
    modules.sort_by_key(|module| std::cmp::Reverse(module.close));
    for module in modules {
        let mut generated = String::new();
        let mut origins = Vec::new();
        let mut placed: Vec<(&str, u32, u32)> = Vec::new();
        for segment in &module.data {
            let form = &expanded.text[segment.start..=segment.end];
            let (file, line) = expanded.origin(segment.line);
            let described = format!("{} at {}:{line}", segment.name, file.display());
            let address = data_address(form, &described)?;
            let length = data_length(form, &described)?;
            for (other, other_address, other_length) in &placed {
                if address < other_address + other_length && *other_address < address + length {
                    return fail(format!(
                        "data segment `{}` overlaps `{other}`: {address}..{} vs {other_address}..{}",
                        segment.name,
                        address + length,
                        other_address + other_length
                    ));
                }
            }
            placed.push((&segment.name, address, length));
            generated.push_str(&format!(
                "  (global {name}.ptr i32 (i32.const {address})) (global {name}.len i32 (i32.const {length}))\n",
                name = segment.name,
            ));
            origins.push((file, line));
        }

        // Insert at the closing paren's exact byte, never at a line boundary: a
        // line boundary can fall inside a multi-line form.
        let close = module.close;
        let line_of_close = expanded.text[..close].matches('\n').count() + 1;
        expanded.text = format!(
            "{}\n{generated}{}",
            &expanded.text[..close],
            &expanded.text[close..]
        );
        // The closing paren's line becomes a prefix line, the generated lines,
        // and a remainder line; prefix and remainder keep the original origin.
        let carried = expanded.origin(line_of_close);
        origins.push(carried);
        expanded
            .origins
            .splice(line_of_close..line_of_close, origins);
    }
    Ok(())
}

/// The literal address of a data segment. A named segment must place itself at
/// a constant, because the generated `.ptr` global has to hold that constant.
fn data_address(form: &str, described: &str) -> Result<u32> {
    let Some(rest) = form.split("i32.const").nth(1) else {
        return fail(format!(
            "data segment `{described}` needs a literal `(i32.const <addr>)` offset"
        ));
    };
    let token: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == 'x')
        .filter(|c| *c != '_')
        .collect();
    let parsed = match token.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => token.parse(),
    };
    parsed.map_err(|_| {
        wasmtime::Error::msg(format!(
            "data segment `{described}` has an offset `{token}` that is not a literal i32"
        ))
    })
}

/// Decoded byte length of every string literal in a data segment.
fn data_length(form: &str, described: &str) -> Result<u32> {
    let bytes = form.as_bytes();
    let mut total = 0u32;
    let mut i = 0usize;
    let mut block = 0usize;
    while i < bytes.len() {
        if block > 0 {
            if bytes[i] == b'(' && bytes.get(i + 1) == Some(&b';') {
                block += 1;
                i += 2;
            } else if bytes[i] == b';' && bytes.get(i + 1) == Some(&b')') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b';' if bytes.get(i + 1) == Some(&b';') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if bytes.get(i + 1) == Some(&b';') => {
                block = 1;
                i += 2;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    let (bytes_used, produced) = escape_len(&bytes[i..], described)?;
                    i += bytes_used;
                    total += produced;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok(total)
}

/// How many source bytes one character of a WAT string literal consumes, and
/// how many bytes it contributes to the segment. The escape set is closed: an
/// unrecognised escape is a lex error, so refusing here beats guessing a length.
fn escape_len(rest: &[u8], described: &str) -> Result<(usize, u32)> {
    if rest[0] != b'\\' {
        return Ok((1, 1));
    }
    match rest.get(1) {
        Some(b't' | b'n' | b'r' | b'"' | b'\'' | b'\\') => Ok((2, 1)),
        Some(b'u') => {
            let close = rest
                .iter()
                .position(|b| *b == b'}')
                .ok_or_else(|| unknown_escape(described))?;
            let hex =
                std::str::from_utf8(&rest[3..close]).map_err(|_| unknown_escape(described))?;
            let scalar =
                u32::from_str_radix(hex.trim(), 16).map_err(|_| unknown_escape(described))?;
            let ch = char::from_u32(scalar).ok_or_else(|| unknown_escape(described))?;
            Ok((close + 1, ch.len_utf8() as u32))
        }
        Some(digit) if digit.is_ascii_hexdigit() => {
            if rest.get(2).is_some_and(u8::is_ascii_hexdigit) {
                Ok((3, 1))
            } else {
                Err(unknown_escape(described))
            }
        }
        _ => Err(unknown_escape(described)),
    }
}

fn unknown_escape(described: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!(
        "data segment `{described}` has an escape this harness cannot measure; \
         a named segment's length must be exact"
    ))
}

#[cfg(test)]
mod tests {
    use super::{address, data_address, data_length, has_offset};

    #[test]
    fn data_addresses_accept_decimal_and_hex() {
        assert_eq!(
            data_address("(data $a (i32.const 1024) \"x\")", "a").unwrap(),
            1024
        );
        assert_eq!(
            data_address("(data $a (i32.const 0x1000) \"x\")", "a").unwrap(),
            4096
        );
        assert!(data_address("(data $a (global.get $base) \"x\")", "a").is_err());
    }

    #[test]
    fn an_offset_inside_the_data_itself_is_not_a_placement() {
        assert!(has_offset("(data $a (i32.const 0x100) \"x\")"));
        assert!(!has_offset("(data $a \"x\")"));
        // The literal mentions the instruction; the segment still has no offset.
        assert!(!has_offset("(data $a \"use (i32.const 0) here\")"));
    }

    #[test]
    fn addresses_parse_as_decimal_or_hex() {
        assert_eq!(address("0x1000", "seg").unwrap(), 0x1000);
        assert_eq!(address("4096", "seg").unwrap(), 4096);
        assert_eq!(address("0x1_000", "seg").unwrap(), 0x1000);
        assert!(address("nope", "seg").is_err());
    }

    #[test]
    fn data_lengths_count_decoded_bytes() {
        let count = |form: &str| data_length(form, "seg").unwrap();
        assert_eq!(count("(data $a (i32.const 0) \"abc\")"), 3);
        // Concatenated literals are one segment.
        assert_eq!(count("(data $a (i32.const 0) \"ab\" \"c\")"), 3);
        // Escapes are one byte each, whatever their source length.
        assert_eq!(count("(data $a (i32.const 0) \"a\\nb\\tc\")"), 5);
        assert_eq!(count("(data $a (i32.const 0) \"\\1b[0m\")"), 4);
        assert_eq!(count("(data $a (i32.const 0) \"\\\"\\\\\")"), 2);
        // Multi-byte characters count as their UTF-8 length, written either way.
        assert_eq!(count("(data $a (i32.const 0) \"\u{25c6}\")"), 3);
        assert_eq!(count("(data $a (i32.const 0) \"\\u{25c6}\")"), 3);
        // A comment inside the form contributes nothing, quotes included.
        assert_eq!(
            count("(data $a (i32.const 0) ;; \"not data\"\n  \"ok\")"),
            2
        );
    }

    #[test]
    fn unmeasurable_escapes_are_refused_rather_than_guessed() {
        assert!(data_length("(data $a (i32.const 0) \"\\q\")", "seg").is_err());
        assert!(data_length("(data $a (i32.const 0) \"\\f\")", "seg").is_err());
    }
}
