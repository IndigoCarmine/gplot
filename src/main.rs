mod data;
mod decimate;

use data::{Dataset, Stats};
use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints, PlotUi};
use std::path::PathBuf;

const USAGE: &str = "usage: gplot <file>\n\n\
    Plots an xmgrace/GROMACS .xvg or PLUMED COLVAR file.";

/// Cap for series whose x isn't sorted (and so can't be pixel-bucketed). Keeps
/// several such series well under wgpu's 256 MB index buffer limit.
const MAX_POINTS_PER_SERIES: usize = 200_000;

fn main() -> eframe::Result {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    if let Some("-h" | "--help") = path.to_str() {
        println!("{USAGE}");
        return Ok(());
    }

    let data = match Dataset::load(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("gplot: {e}");
            std::process::exit(1);
        }
    };

    let title = if data.title.is_empty() {
        format!("gplot — {}", path.display())
    } else {
        format!("{} — {}", data.title, path.display())
    };

    eframe::run_native(
        "gplot",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1000.0, 700.0])
                .with_title(title),
            ..Default::default()
        },
        Box::new(move |_cc| Ok(Box::new(App::new(path, data)))),
    )
}

struct App {
    path: PathBuf,
    data: Dataset,
    x_col: usize,
    /// Whether rows are sorted by `x_col`; enables pixel-bucketed decimation.
    x_monotonic: bool,
    /// Per-column visibility, parallel to `data.columns`.
    shown: Vec<bool>,
    /// Per-column stats; the data never changes, so compute them once.
    stats: Vec<Option<Stats>>,
    /// Screen rect of the plot, captured each frame so exports can crop to it.
    plot_rect: Option<egui::Rect>,
    /// Set while a screenshot is in flight; the captured image lands there.
    pending_export: Option<PathBuf>,
    status: String,
}

impl App {
    fn new(path: PathBuf, data: Dataset) -> Self {
        // Default: x is column 0, every other column plotted.
        let shown = (0..data.columns.len()).map(|c| c != 0).collect();
        let stats = (0..data.columns.len()).map(|c| Stats::of(data.column(c))).collect();
        let x_monotonic = decimate::is_monotonic(data.column(0));
        App {
            path,
            data,
            x_col: 0,
            x_monotonic,
            shown,
            stats,
            plot_rect: None,
            pending_export: None,
            status: String::new(),
        }
    }

    fn y_columns(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.data.columns.len()).filter(|&c| c != self.x_col && self.shown[c])
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_screenshot(&ctx);

        egui::Panel::top("controls").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("X axis:");
                let prev_x = self.x_col;
                egui::ComboBox::from_id_salt("x_col")
                    .selected_text(&self.data.columns[self.x_col])
                    .show_ui(ui, |ui| {
                        for c in 0..self.data.columns.len() {
                            ui.selectable_value(&mut self.x_col, c, &self.data.columns[c]);
                        }
                    });
                if self.x_col != prev_x {
                    self.x_monotonic = decimate::is_monotonic(self.data.column(self.x_col));
                }
                ui.separator();
                ui.label("Y series:");
                for c in 0..self.data.columns.len() {
                    if c != self.x_col {
                        ui.toggle_value(&mut self.shown[c], &self.data.columns[c]);
                    }
                }
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("stats").show(ui, |ui| {
            ui.add_space(4.0);
            self.stats_table(ui);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save plot as PNG").clicked() {
                    self.start_export(&ctx);
                }
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let x_name = self.data.columns[self.x_col].clone();
            let y_name = self.data.yaxis_label.clone();
            let response = Plot::new("plot")
                .legend(Legend::default())
                .x_axis_label(x_name)
                .y_axis_label(y_name)
                .show(ui, |plot_ui| {
                    for c in self.y_columns() {
                        let points = PlotPoints::new(self.series_points(plot_ui, c));
                        plot_ui.line(Line::new(self.data.columns[c].clone(), points));
                    }
                });
            self.plot_rect = Some(response.response.rect);
        });
    }
}

impl App {
    /// Points for series `c`, decimated to about what the plot can show.
    fn series_points(&self, plot_ui: &PlotUi, c: usize) -> Vec<[f64; 2]> {
        let rows = &self.data.rows;
        let x = self.x_col;
        let buckets = plot_ui.transform().frame().width().ceil().max(1.0) as usize;

        if rows.len() <= 4 * buckets {
            return rows.iter().map(|r| [r[x], r[c]]).filter(|[x, y]| x.is_finite() && y.is_finite()).collect();
        }
        if !self.x_monotonic {
            return decimate::stride(rows, x, c, MAX_POINTS_PER_SERIES);
        }

        // Sorted x: cull to the visible range (plus one row each side so the line
        // runs to the edge), then keep min/max per pixel column. While auto-fitting,
        // use everything so the fit still sees the whole series.
        let (data_lo, data_hi) = (rows[0][x], rows[rows.len() - 1][x]);
        let bounds = plot_ui.plot_bounds();
        let (lo, hi) = (bounds.min()[0], bounds.max()[0]);
        let (lo, hi) = if plot_ui.auto_bounds().x || !(lo.is_finite() && hi.is_finite() && hi > lo) {
            (data_lo, data_hi)
        } else {
            (lo, hi)
        };
        let start = rows.partition_point(|r| r[x] < lo).saturating_sub(1);
        let end = (rows.partition_point(|r| r[x] <= hi) + 1).min(rows.len());
        decimate::m4(&rows[start..end], x, c, lo, hi, buckets)
    }

    fn stats_table(&self, ui: &mut egui::Ui) {
        egui::Grid::new("stats").striped(true).num_columns(7).show(ui, |ui| {
            for h in ["series", "n", "mean", "std", "min", "max", "median"] {
                ui.strong(h);
            }
            ui.end_row();

            for c in std::iter::once(self.x_col).chain(self.y_columns()) {
                let Some(s) = &self.stats[c] else { continue };
                ui.label(&self.data.columns[c]);
                ui.label(s.n.to_string());
                for v in [s.mean, s.std, s.min, s.max, s.median] {
                    ui.label(format!("{v:.4}"));
                }
                ui.end_row();
            }
        });
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        self.pending_export = Some(self.path.with_extension("png"));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }

    /// Picks up the frame captured by `ViewportCommand::Screenshot`, crops it to
    /// the plot area, and writes it out.
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let (Some(image), Some(path)) = (shot, self.pending_export.take()) else {
            return;
        };

        // Crop to the plot area if we captured it; otherwise export the whole frame.
        let cropped = match self.plot_rect {
            Some(region) => image.region(&region, Some(ctx.pixels_per_point())),
            None => (*image).clone(),
        };
        self.status = match save_png(&path, &cropped) {
            Ok(()) => format!("saved {}", path.display()),
            Err(e) => format!("save failed: {e}"),
        };
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let raw: Vec<u8> = image.as_raw().to_vec();
    let buf = image::RgbaImage::from_raw(w, h, raw).ok_or("image buffer size mismatch")?;
    buf.save(path).map_err(|e| e.to_string())
}
