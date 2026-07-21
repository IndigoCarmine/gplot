# gplot

A quick, xmgrace-style plotter for GROMACS `.xvg` and PLUMED `COLVAR` files,
built with [egui](https://github.com/emilk/egui). Point it at a data file and it
opens an interactive window where you can pick axes, toggle series, read basic
statistics, and export the plot as a PNG.

## Features

- **Reads GROMACS `.xvg`** — parses `@ title`, `@ xaxis/yaxis label`, and
  `@ sN legend` headers to name the axes and series automatically.
- **Reads PLUMED `COLVAR`** — uses the `#! FIELDS ...` header for column names.
- **Interactive plot** — zoom, pan, and a legend, courtesy of `egui_plot`.
- **Choose your axes** — pick which column is the X axis; toggle any Y series on/off.
- **Summary statistics** — n, mean, std (sample), min, max, and median for each
  shown column, updated live.
- **PNG export** — "Save plot as PNG" writes `<inputfile>.png` next to your data.

## Install

Requires a recent Rust toolchain (install via [rustup](https://rustup.rs/)).

```bash
git clone <this-repo> gplot
cd gplot
cargo install --path .
```

This builds an optimized binary and installs it to `~/.cargo/bin/gplot`, which
rustup puts on your `PATH`. To update after pulling new changes, re-run
`cargo install --path . --force`.

## Usage

```bash
gplot hbnum.xvg      # GROMACS xvg
gplot COLVAR         # PLUMED colvar
gplot --help
```

The file argument is required. In the window:

- **X axis** — dropdown to choose the independent column.
- **Y series** — toggle buttons for each remaining column.
- The **statistics table** at the bottom reflects the currently shown columns.
- **Save plot as PNG** — writes `<inputfile>.png` alongside the input file.

### Running from source without installing

```bash
cargo run -- hbnum.xvg
```

Note the `--`: it tells cargo to pass the filename to `gplot` rather than
interpreting it as a cargo argument.

## Supported file formats

| Format          | Detection                     | Column names from            |
| --------------- | ----------------------------- | ---------------------------- |
| GROMACS `.xvg`  | default                       | `@ xaxis label`, `@ sN legend` |
| PLUMED `COLVAR` | first line starts with `#! FIELDS` | the `FIELDS` list        |

Comment lines (`#`) and xmgrace directives (`@`) are ignored for data; ragged
rows are padded with `NaN` and skipped in the plot and statistics.

## Development

```bash
cargo test      # parser + statistics unit tests
cargo run -- <file>
```
