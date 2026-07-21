use std::path::Path;

/// A table of numeric columns loaded from an xmgrace/GROMACS `.xvg` file or a
/// PLUMED COLVAR file.
pub struct Dataset {
    pub title: String,
    pub yaxis_label: String,
    /// Column names, one per column of `rows`.
    pub columns: Vec<String>,
    /// `rows[r][c]` — every row has `columns.len()` entries.
    pub rows: Vec<Vec<f64>>,
}

impl Dataset {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// Detects the format from the header: PLUMED files start with `#! FIELDS`,
    /// everything else is treated as xvg.
    pub fn parse(text: &str) -> Result<Self, String> {
        if plumed_fields(text).is_some() {
            Self::parse_plumed(text)
        } else {
            Self::parse_xvg(text)
        }
    }

    fn parse_xvg(text: &str) -> Result<Self, String> {
        let mut title = String::new();
        let mut xaxis_label = String::new();
        let mut yaxis_label = String::new();
        let mut legends: Vec<(usize, String)> = Vec::new();
        let mut rows: Vec<Vec<f64>> = Vec::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(cmd) = line.strip_prefix('@') {
                parse_xvg_command(cmd.trim(), &mut title, &mut xaxis_label, &mut yaxis_label, &mut legends);
                continue;
            }
            if let Some(row) = parse_row(line) {
                rows.push(row);
            }
        }

        let ncols = normalize(&mut rows)?;
        let mut columns = Vec::with_capacity(ncols);
        columns.push(if xaxis_label.is_empty() { "x".to_owned() } else { xaxis_label });
        for i in 0..ncols - 1 {
            let named = legends.iter().find(|(idx, _)| *idx == i).map(|(_, n)| n.clone());
            columns.push(named.unwrap_or_else(|| format!("s{i}")));
        }

        Ok(Dataset { title, yaxis_label, columns, rows })
    }

    fn parse_plumed(text: &str) -> Result<Self, String> {
        let mut columns: Vec<String> = plumed_fields(text).ok_or("missing `#! FIELDS` header")?;
        let mut rows: Vec<Vec<f64>> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(row) = parse_row(line) {
                rows.push(row);
            }
        }

        // Trust the data width over the header if the two disagree.
        let ncols = normalize(&mut rows)?;
        columns.truncate(ncols);
        for i in columns.len()..ncols {
            columns.push(format!("col{i}"));
        }

        Ok(Dataset { title: String::new(), yaxis_label: String::new(), columns, rows })
    }

    pub fn column(&self, c: usize) -> impl Iterator<Item = f64> + '_ {
        self.rows.iter().map(move |r| r[c])
    }
}

/// Returns the field names of a PLUMED `#! FIELDS ...` header, if present.
fn plumed_fields(text: &str) -> Option<Vec<String>> {
    text.lines()
        .find(|l| l.trim_start().starts_with("#!"))
        .and_then(|l| l.trim_start().trim_start_matches("#!").trim().strip_prefix("FIELDS"))
        .map(|rest| rest.split_whitespace().map(str::to_owned).collect())
        .filter(|f: &Vec<String>| !f.is_empty())
}

fn parse_row(line: &str) -> Option<Vec<f64>> {
    let row: Vec<f64> = line.split_whitespace().filter_map(|tok| tok.parse::<f64>().ok()).collect();
    (!row.is_empty()).then_some(row)
}

/// Pads ragged rows with NaN so indexing is always safe; returns the width.
fn normalize(rows: &mut [Vec<f64>]) -> Result<usize, String> {
    let ncols = rows.iter().map(Vec::len).max().ok_or("no data rows found")?;
    for row in rows.iter_mut() {
        row.resize(ncols, f64::NAN);
    }
    Ok(ncols)
}

fn parse_xvg_command(
    cmd: &str,
    title: &mut String,
    xaxis: &mut String,
    yaxis: &mut String,
    legends: &mut Vec<(usize, String)>,
) {
    let quoted = || cmd.split('"').nth(1).unwrap_or("").to_owned();

    if let Some(rest) = cmd.strip_prefix("title") {
        if rest.trim_start().starts_with('"') {
            *title = quoted();
        }
    } else if let Some(rest) = cmd.strip_prefix("xaxis") {
        if rest.trim_start().starts_with("label") {
            *xaxis = quoted();
        }
    } else if let Some(rest) = cmd.strip_prefix("yaxis") {
        if rest.trim_start().starts_with("label") {
            *yaxis = quoted();
        }
    } else if cmd.starts_with('s') {
        // `s0 legend "Hydrogen bonds"`
        let mut parts = cmd.split_whitespace();
        let Some(sid) = parts.next() else { return };
        if parts.next() != Some("legend") {
            return;
        }
        if let Ok(idx) = sid[1..].parse::<usize>() {
            let name = quoted();
            if !name.is_empty() {
                legends.push((idx, name));
            }
        }
    }
}

/// Summary statistics over a single column.
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
    pub median: f64,
}

impl Stats {
    pub fn of(values: impl Iterator<Item = f64>) -> Option<Stats> {
        let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(f64::total_cmp);
        let n = v.len();
        let mean = v.iter().sum::<f64>() / n as f64;
        // Sample standard deviation; undefined for a single point.
        let std = if n > 1 {
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
        } else {
            0.0
        };
        let median = if n % 2 == 0 { (v[n / 2 - 1] + v[n / 2]) / 2.0 } else { v[n / 2] };
        Some(Stats { n, mean, std, min: v[0], max: v[n - 1], median })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XVG: &str = r#"
# comment
@    title "Hydrogen Bonds"
@    xaxis  label "Time (ps)"
@    yaxis  label "Number"
@ s0 legend "Hydrogen bonds"
@ s1 legend "Pairs within 0.35 nm"
    0   388   2408
  100   384   2275
  200   366   2320
"#;

    #[test]
    fn parses_xvg_header_and_data() {
        let d = Dataset::parse(XVG).unwrap();
        assert_eq!(d.title, "Hydrogen Bonds");
        assert_eq!(d.columns, ["Time (ps)", "Hydrogen bonds", "Pairs within 0.35 nm"]);
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[2], [200.0, 366.0, 2320.0]);
    }

    #[test]
    fn parses_plumed_colvar() {
        let d = Dataset::parse("#! FIELDS time d\n 0.000000 4.486957\n 100.000005 4.429317\n").unwrap();
        assert_eq!(d.columns, ["time", "d"]);
        assert_eq!(d.rows.len(), 2);
        assert_eq!(d.rows[0][1], 4.486957);
    }

    #[test]
    fn stats_match_by_hand() {
        let s = Stats::of([1.0, 2.0, 3.0, 4.0].into_iter()).unwrap();
        assert_eq!(s.n, 4);
        assert_eq!(s.mean, 2.5);
        assert_eq!(s.median, 2.5);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 4.0);
        assert!((s.std - 1.290_994).abs() < 1e-5);
    }

    #[test]
    fn unnamed_columns_get_fallback_names() {
        let d = Dataset::parse("1 2 3\n4 5 6\n").unwrap();
        assert_eq!(d.columns, ["x", "s0", "s1"]);
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(Dataset::parse("# only a comment\n").is_err());
    }
}
