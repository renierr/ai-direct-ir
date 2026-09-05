//! A minimal WAT reader: enough structure to find module boundaries, function
//! index order, and named data segments without parsing the whole grammar.

/// A named data segment, plus the byte range of its whole form.
pub(super) struct DataSegment {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// One Core WASM module found in the source: a plain `(module ...)` app, or a
/// `(core module ...)` inside a component.
pub(super) struct ModuleScan {
    start: usize,
    /// Byte index of the paren closing this module.
    pub(super) close: usize,
    /// Source line of every function in Core index order: imported functions
    /// first, then the ones defined in the body.
    pub(super) functions: Vec<usize>,
    pub(super) data: Vec<DataSegment>,
}

/// One pass over a WAT source: enough structure to translate validator output
/// and to generate the address/length globals for named data segments.
/// Comments and string literals are skipped, so `(func` or `(data` inside them
/// never counts.
pub(super) struct Scan {
    /// Modules in source order, which is also Core module index order.
    pub(super) modules: Vec<ModuleScan>,
}

/// A form the scanner has entered but not yet left.
struct Frame<'a> {
    head: &'a str,
    name: Option<String>,
    line: usize,
    start: usize,
    imported: Vec<usize>,
    defined: Vec<usize>,
    data: Vec<DataSegment>,
}

impl<'a> Frame<'a> {
    fn new(head: &'a str, name: Option<String>, line: usize, start: usize) -> Self {
        Frame {
            head,
            name,
            line,
            start,
            imported: Vec::new(),
            defined: Vec::new(),
            data: Vec::new(),
        }
    }
}

pub(super) fn scan_module(text: &str) -> Scan {
    let bytes = text.as_bytes();
    let mut open: Vec<Frame<'_>> = Vec::new();
    let mut scan = Scan {
        modules: Vec::new(),
    };
    let mut line = 1usize;
    let mut block = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
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
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'(' => {
                let start = i;
                i += 1;
                let mut head = &text[i..i + token_len(&bytes[i..])];
                i += head.len();
                // `core module`, `core func`, `core instance`: the second word
                // is what identifies the form.
                if head == "core" {
                    let skipped = leading_space(&bytes[i..]);
                    let after = i + skipped;
                    let len = token_len(&bytes[after..]);
                    if len > 0 {
                        head = &text[after..after + len];
                        line += bytes[i..after].iter().filter(|b| **b == b'\n').count();
                        i = after + len;
                    }
                }
                let depth = open.len();
                if head == "func" {
                    if depth >= 1 && open[depth - 1].head == "module" {
                        open[depth - 1].defined.push(line);
                    } else if depth >= 2
                        && open[depth - 1].head == "import"
                        && open[depth - 2].head == "module"
                    {
                        open[depth - 2].imported.push(line);
                    }
                }
                // Only a named segment directly inside a module opts in.
                let name = if head == "data" && depth >= 1 && open[depth - 1].head == "module" {
                    identifier(text, &bytes[i..], i)
                } else {
                    None
                };
                open.push(Frame::new(head, name, line, start));
            }
            b')' => {
                if let Some(frame) = open.pop() {
                    if let Some(name) = frame.name {
                        if let Some(parent) = open.last_mut() {
                            parent.data.push(DataSegment {
                                name,
                                line: frame.line,
                                start: frame.start,
                                end: i,
                            });
                        }
                    }
                    if frame.head == "module" {
                        let mut functions = frame.imported;
                        functions.extend(frame.defined);
                        scan.modules.push(ModuleScan {
                            start: frame.start,
                            close: i,
                            functions,
                            data: frame.data,
                        });
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    scan.modules.sort_by_key(|module| module.start);
    scan
}

/// Length of the token starting at `bytes[0]`, stopping at any WAT delimiter.
pub(super) fn token_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b';' | b'"'))
        .unwrap_or(bytes.len())
}

fn leading_space(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len())
}

/// The `$name` immediately following a form's head keyword, if there is one.
fn identifier(text: &str, rest: &[u8], offset: usize) -> Option<String> {
    let skipped = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    if rest[skipped] != b'$' {
        return None;
    }
    let start = offset + skipped;
    let len = token_len(&rest[skipped..]);
    Some(text[start..start + len].to_string())
}

#[cfg(test)]
mod tests {
    use super::scan_module;

    #[test]
    fn scanned_functions_follow_core_index_order() {
        let text = "(module
  ;; (func in a comment must not count)
  (import \"wasi\" \"fd_write\"
    (func $write (param i32) (result i32)))
  (type $t (func (param i32)))
  (func $first (result i32) (i32.const 1))
  (data (i32.const 0) \"(func in a string)\")
  (func $second)
)
";
        // Imported functions come first in the index space, then definitions.
        assert_eq!(scan_module(text).modules[0].functions, vec![4, 6, 8]);
    }

    #[test]
    fn scanned_functions_ignore_block_comments() {
        let text = "(module (; (func hidden) ;) (func $only))\n";
        assert_eq!(scan_module(text).modules[0].functions, vec![1]);
    }

    #[test]
    fn scanned_data_segments_are_named_and_bounded() {
        let text = "\
(module
  (data (i32.const 0) \"unnamed stays untouched\")
  (data $banner (i32.const 4096) \"hi\")
)
";
        let scan = scan_module(text);
        let data = &scan.modules[0].data;
        assert_eq!(data.len(), 1, "only a named segment opts in");
        assert_eq!(data[0].name, "$banner");
        assert_eq!(data[0].line, 3);
        let form = &text[data[0].start..=data[0].end];
        assert_eq!(form, "(data $banner (i32.const 4096) \"hi\")");
    }
}
