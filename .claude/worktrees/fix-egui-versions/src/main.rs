mod data;

use data::{Dataset, Stats};
use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use std::path::PathBuf;

const USAGE: &str = "usage: gplot <file>\n\n\
    Plots an xmgrace/GROMACS .xvg or PLUMED COLVAR file.";

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
    /// Per-column visibility, parallel to `data.columns`.
    shown: Vec<bool>,
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
        App { path, data, x_col: 0, shown, plot_rect: None, pending_export: None, status: String::new() }
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
                egui::ComboBox::from_id_salt("x_col")
                    .selected_text(&self.data.columns[self.x_col])
                    .show_ui(ui, |ui| {
                        for c in 0..self.data.columns.len() {
                            ui.selectable_value(&mut self.x_col, c, &self.data.columns[c]);
                        }
                    });
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
                        let points: PlotPoints = self
                            .data
                            .rows
                            .iter()
                            .map(|r| [r[self.x_col], r[c]])
                            .filter(|[x, y]| x.is_finite() && y.is_finite())
                            .collect();
                        plot_ui.line(Line::new(self.data.columns[c].clone(), points));
                    }
                });
            self.plot_rect = Some(response.response.rect);
        });
    }
}

impl App {
    fn stats_table(&self, ui: &mut egui::Ui) {
        egui::Grid::new("stats").striped(true).num_columns(7).show(ui, |ui| {
            for h in ["series", "n", "mean", "std", "min", "max", "median"] {
                ui.strong(h);
            }
            ui.end_row();

            for c in std::iter::once(self.x_col).chain(self.y_columns()) {
                let Some(s) = Stats::of(self.data.column(c)) else { continue };
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
