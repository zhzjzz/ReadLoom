use crate::domain::text_document::LineEnding;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEndingAnalysis {
    pub detected: LineEnding,
    pub primary: LineEnding,
    pub normalized: String,
}

pub fn analyze_and_normalize(content: &str) -> LineEndingAnalysis {
    let bytes = content.as_bytes();
    let mut crlf = 0usize;
    let mut lf = 0usize;
    let mut cr = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 2;
            }
            b'\r' => {
                cr += 1;
                index += 1;
            }
            b'\n' => {
                lf += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }

    let kinds = usize::from(crlf > 0) + usize::from(lf > 0) + usize::from(cr > 0);
    let detected = match kinds {
        0 => LineEnding::None,
        1 if crlf > 0 => LineEnding::Crlf,
        1 if lf > 0 => LineEnding::Lf,
        1 => LineEnding::Cr,
        _ => LineEnding::Mixed,
    };
    let primary = [
        (LineEnding::Crlf, crlf),
        (LineEnding::Lf, lf),
        (LineEnding::Cr, cr),
    ]
    .into_iter()
    .max_by_key(|(_, count)| *count)
    .filter(|(_, count)| *count > 0)
    .map(|(line_ending, _)| line_ending)
    .unwrap_or(LineEnding::None);

    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    LineEndingAnalysis {
        detected,
        primary,
        normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_crlf_lf_cr_mixed_and_none() {
        assert_eq!(
            analyze_and_normalize("a\r\nb\r\n").detected,
            LineEnding::Crlf
        );
        assert_eq!(analyze_and_normalize("a\nb\n").detected, LineEnding::Lf);
        assert_eq!(analyze_and_normalize("a\rb\r").detected, LineEnding::Cr);
        let mixed = analyze_and_normalize("a\r\nb\nc\r");
        assert_eq!(mixed.detected, LineEnding::Mixed);
        assert_eq!(mixed.normalized, "a\nb\nc\n");
        assert_eq!(
            analyze_and_normalize("single line").detected,
            LineEnding::None
        );
    }

    #[test]
    fn mixed_primary_line_ending_uses_the_most_common_kind() {
        let analysis = analyze_and_normalize("a\r\nb\r\nc\n");
        assert_eq!(analysis.detected, LineEnding::Mixed);
        assert_eq!(analysis.primary, LineEnding::Crlf);
    }
}
