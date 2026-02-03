use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::ops::Rem;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    thread,
};
use walkdir::WalkDir;

type Md5 = String;

/// ======================
/// OBSERVER EVENTS
/// ======================

#[derive(Debug)]
enum ScanEvent {
    FileScanned,
    Duplicate { hash: Md5, path: PathBuf },
    First { hash: Md5, path: PathBuf },
    Finished,
}

/// ======================
/// DATA MODEL
/// ======================

#[derive(Clone, Debug)]
struct DuplicateFile {
    parent: PathBuf,
    path: PathBuf,
    file_name: PathBuf,
    marked: bool,
}

type FolderGroup = BTreeMap<PathBuf, Vec<DuplicateFile>>;

#[derive(Debug)]
struct DuplicateSet {
    duplicates: FolderGroup,
    first_occurrence: DuplicateFile,
}

/// ======================
/// BACKEND
/// ======================

fn md5sum(path: &Path) -> io::Result<Md5> {
    let mut file = fs::File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(format!("{:x}", md5::compute(buf)))
}

#[derive(Debug)]
enum ScanModus {
    PrioritizeFolders,
    RestOfFolders,
    NonPrioritizedFolders,
}
fn scan_for_duplicates(
    root: &PathBuf,
    exclude_folders: &Vec<PathBuf>,
    prioritize_folders: &Vec<PathBuf>,
    unprioritize_folders: &Vec<PathBuf>,
    scan_modus: ScanModus,
    min_size: u64,
    tx: Sender<ScanEvent>,
    seen: &mut HashMap<Md5, Vec<PathBuf>>,
) {
    //  let mut seen: HashMap<Md5, Vec<PathBuf>> = HashMap::new();

    println!("scanning folder: {:?}", root);
    println!("scanning exclude_folders: {:?}", exclude_folders);
    println!("scanning prioritize_folders: {:?}", prioritize_folders);
    println!("scanning unprioritize_folders: {:?}", unprioritize_folders);

    // entries.into_iter().for_each(|entry| {
    //     println!("wald_dir {:?}", entry);
    // });

    let entries3 = WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    println!("Total {} entries3 found.", entries3.len());

    println!("{}", "_".repeat(80));
    println!("BEFORE entries to process:  {}", entries3.len());
    entries3.iter().for_each(|entry| {
        println!("BEFORE entry:   {:?}", entry.path());
    });

    let entries = entries3
        .into_iter()
        .filter(|e| {
            let path_buf = e.path().to_path_buf();
            !exclude_folders.iter().any(|p| path_buf.starts_with(&p))
        })
        .collect::<Vec<_>>();
    println!("AFTER entries to process : {:?}", entries.len());
    entries.iter().for_each(|entry| {
        println!("AFTER entry:   {:?}", entry.path());
    });
    //
    //
    // println!("{}", "_".repeat(80));
    // println!("entries after excluding: {:?}", entries);
    let entries: Vec<walkdir::DirEntry> = match scan_modus {
        ScanModus::PrioritizeFolders => entries
            .into_iter()
            .filter(|e| {
                let path_buf = e.path().to_path_buf();
                prioritize_folders.iter().any(|p| path_buf.starts_with(&p))
            })
            .collect(),
        ScanModus::RestOfFolders => entries
            .into_iter()
            .filter(|e| {
                let path_buf = e.path().to_path_buf();
                !prioritize_folders.iter().any(|p| path_buf.starts_with(&p))
                    && !unprioritize_folders
                        .iter()
                        .any(|p| path_buf.starts_with(&p))
            })
            .collect(),
        ScanModus::NonPrioritizedFolders => entries
            .into_iter()
            .filter(|e| {
                let path_buf = e.path().to_path_buf();
                unprioritize_folders
                    .iter()
                    .any(|p| path_buf.starts_with(&p))
            })
            .collect(),
    };

    println!("Scanning {} entries.", entries.len());
    let cnt = entries.len();

    for (idx, entry) in entries.into_iter().enumerate() {
        if !entry.file_type().is_file() {
            //  println!("skipping file: {}", entry.path().to_str().unwrap());
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if min_size > 0 && meta.len() < min_size {
            continue;
        }

        if idx.rem(50) == 0 {
            println!("processing {} / {}", idx + 1, cnt);
        }

        if let Ok(hash) = md5sum(entry.path()) {
            let list = seen.entry(hash.clone()).or_default();
            list.push(entry.path().to_path_buf());
            // Emit duplicates only when we KNOW it's a duplicate
            if list.len() == 2 {
                let first = list.first().unwrap();
                let first = ScanEvent::First {
                    hash: hash.clone(),
                    path: first.to_path_buf(),
                };
                let _ = tx.send(first);

                let dup_event = ScanEvent::Duplicate {
                    hash,
                    path: entry.path().to_path_buf(),
                };
                let _ = tx.send(dup_event);
            } else if list.len() > 2 {
                let x_duplicate = ScanEvent::Duplicate {
                    hash,
                    path: entry.path().to_path_buf(),
                };
                let _ = tx.send(x_duplicate);
            }
        }

        let _ = tx.send(ScanEvent::FileScanned);
    }

    let _ = tx.send(ScanEvent::Finished);
}

/// ======================
/// MOVE MARKED FILES
/// ======================

fn move_marked(
    sets: &mut HashMap<Md5, DuplicateSet>,
    base: &Path,
    target: &Path,
) -> io::Result<()> {
    for set in sets.values_mut() {
        for files in set.duplicates.values_mut() {
            for file in files.iter_mut().filter(|f| f.marked) {
                let rel = file.path.strip_prefix(base).unwrap();
                let dest = target.join(rel);

                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::rename(&file.path, &dest)?;
                file.marked = false;
            }
        }
    }
    Ok(())
}

fn move_marked2(
    dups: &mut BTreeMap<PathBuf, DupsView>,
    base: &Path,
    target: &Path,
) -> io::Result<()> {
    for (_, du) in dups.into_iter() {
        for entry in du.duplicates.iter_mut().filter(|ff| ff.marked_for_deletion) {
            let pp = Path::new(&entry.path);
            let rel = pp.strip_prefix(&base).unwrap();
            let dest = target.join(rel);

            if let Some(parent) = dest.parent() {
                println!("creating folder:  ||{}||", parent.display());
                fs::create_dir_all(parent)?;
            }

            println!("moving: \n||{:?}||  -->  ||{:?}||", entry.path, dest);
            fs::rename(&entry.path, &dest)?;
            entry.marked_for_deletion = true;
        }
    }
    Ok(())
}

/// ======================
/// GUI
/// ======================

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    root: String,
    target: String,
    min_size_mb: u64,
    exclude_folders: Vec<String>,
    prioritize_folders: Vec<String>,
    unprioritize_folders: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DupsViewEntry {
    path: String,
    marked_for_deletion: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct DupsView {
    original_file: PathBuf,
    original_path: PathBuf,
    duplicates: Vec<DupsViewEntry>,
    marked_for_deletion: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: "/Users/bumzack/stoff/coding/rust/dups/test_data".to_string(),
            target: "/Users/bumzack/stoff/coding/rust/dups/test_data/to_delete".to_string(),
            min_size_mb: 0,
            exclude_folders: vec![
                "/Users/bumzack/stoff/coding/rust/dups/test_data/to_delete".to_string()
            ],
            prioritize_folders: vec![
                "/Users/bumzack/stoff/coding/rust/dups/test_data/prio".to_string()
            ],
            unprioritize_folders: vec![
                "/Users/bumzack/stoff/coding/rust/dups/test_data/prionon".to_string()
            ],
        }
    }
}
struct DupeApp {
    sets: HashMap<Md5, DuplicateSet>,
    scanned_files: usize,
    rx: Option<Receiver<ScanEvent>>,
    scanning: bool,
    config: Config,
    dups_views: BTreeMap<PathBuf, DupsView>,
}

impl Default for DupeApp {
    fn default() -> Self {
        let config = File::open(Path::new("config.json"));
        let config = match config {
            Ok(file) => serde_json::from_reader(file).unwrap(),
            Err(e) => {
                println!("Failed to read config.json. using defaults: {}", e);
                Config::default()
            }
        };

        println!("config: {:?}", config);
        Self {
            sets: HashMap::new(),
            scanned_files: 0,
            rx: None,
            scanning: false,
            config,
            dups_views: BTreeMap::new(),
        }
    }
}

impl eframe::App for DupeApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🧹 Duplicate Finder (Cross-Folder)");

            // ui.horizontal(|ui| {
            //     ui.label("Scan root:");
            //     ui.text_edit_singleline(&mut self.config.root);
            // });

            // ui.horizontal(|ui| {
            //     ui.label("Move target:");
            //     ui.text_edit_singleline(&mut self.config.target);
            // });

            // ui.horizontal(|ui| {
            //     ui.label("Min file size (MB):");
            //     ui.add(egui::DragValue::new(&mut self.config.min_size_mb).range(0..=10240));
            // });

            if ui.button("🔍 Start scan").clicked() && !self.scanning {
                let (tx, rx) = unbounded();
                self.rx = Some(rx);
                self.sets.clear();
                self.scanned_files = 0;
                self.scanning = true;

                let root = PathBuf::from(self.config.root.clone());
                let min_size = self.config.min_size_mb * 1024 * 1024;

                let prioritize_folders = self
                    .config
                    .prioritize_folders
                    .iter()
                    .map(|s| PathBuf::from(s))
                    .collect();
                let exclude_folders = self
                    .config
                    .exclude_folders
                    .iter()
                    .map(|s| PathBuf::from(s))
                    .collect();
                let unprioritize_folders = self
                    .config
                    .unprioritize_folders
                    .iter()
                    .map(|s| PathBuf::from(s))
                    .collect();

                let config =
                    serde_json::to_string_pretty(&self.config).expect("should serialize config");
                let mut res = File::create("config.json").expect("should open config.json");
                res.write_all(config.as_bytes())
                    .expect("should write config.json");
                res.flush().expect("should flush config.json");

                self.dups_views.clear();

                thread::spawn(move || {
                    let mut seen: HashMap<Md5, Vec<PathBuf>> = HashMap::new();

                    scan_for_duplicates(
                        &root,
                        &exclude_folders,
                        &prioritize_folders,
                        &unprioritize_folders,
                        ScanModus::PrioritizeFolders,
                        min_size,
                        tx.clone(),
                        &mut seen,
                    );
                    scan_for_duplicates(
                        &root,
                        &exclude_folders,
                        &prioritize_folders,
                        &unprioritize_folders,
                        ScanModus::RestOfFolders,
                        min_size,
                        tx.clone(),
                        &mut seen,
                    );
                    scan_for_duplicates(
                        &root,
                        &exclude_folders,
                        &prioritize_folders,
                        &unprioritize_folders,
                        ScanModus::NonPrioritizedFolders,
                        min_size,
                        tx,
                        &mut seen,
                    );
                });
            }

            if let Some(rx) = &self.rx {
                while let Ok(event) = rx.try_recv() {
                    match event {
                        ScanEvent::FileScanned => self.scanned_files += 1,

                        ScanEvent::Duplicate { hash, path } => {
                            let _ = self.sets.entry(hash).and_modify(|e| {
                                let df = DuplicateFile {
                                    path: path.clone(),
                                    marked: false,
                                    parent: PathBuf::from(path.parent().unwrap()),
                                    file_name: PathBuf::from(path.file_name().unwrap()),
                                };
                                //println!("adding duplicate:      {:?}", df.path);
                                e.duplicates.entry(path).or_insert(vec![df]);
                            });
                            // println!("new entry with added duplicate {e:?}");
                            self.dups_views = map_to(&self.sets);
                        }

                        ScanEvent::First { hash, path } => {
                            //  println!("adding first occurrence:      {:?}", path);
                            let first_occurrence = DuplicateFile {
                                path: path.clone(),
                                marked: false,
                                parent: PathBuf::from(path.parent().unwrap()),
                                file_name: PathBuf::from(path.file_name().unwrap()),
                            };

                            self.sets.entry(hash).or_insert_with(|| DuplicateSet {
                                duplicates: BTreeMap::new(),
                                first_occurrence,
                            });

                            self.dups_views = map_to(&self.sets);
                        }

                        ScanEvent::Finished => self.scanning = false,
                    }
                }
            }

            ui.separator();

            ui.vertical(|ui| {
                if self.scanning {
                    ui.label(format!("Scanning… {} files", self.scanned_files));
                } else {
                    if self.scanned_files > 0 {
                        ui.label(format!("Finished scanning {} files", self.scanned_files));
                    } else {
                        ui.label("No files scanned".to_string());
                    }
                }

                if ui.button("🚚 Move marked for deletion").clicked() {
                    let _ = move_marked2(
                        &mut self.dups_views,
                        Path::new(&self.config.root),
                        Path::new(&self.config.target),
                    );
                }
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut idx = 0;
                for (pa, set) in self.dups_views.iter_mut().take(5_000) {
                    idx += 1;
                    let s = format!("{:?}", pa);

                    ui.collapsing(format!("{s}"), |ui| {
                        let s = format!("mark all in folder: {:?}", pa);

                        if ui.checkbox(&mut set.marked_for_deletion, s).changed() {
                            for f in set.duplicates.iter_mut() {
                                f.marked_for_deletion = set.marked_for_deletion;
                            }
                        }
                        for entry in set.duplicates.iter_mut() {
                            idx += 1;

                            let xx = format!("{}", entry.path.clone());
                            ui.checkbox(&mut entry.marked_for_deletion, xx);

                            // ui.indent("duplicates", |ui| {
                            //     for f in files.iter_mut() {
                            //         let display_string = format!(
                            //             "{} @ {}",
                            //             f.file_name.display(),
                            //             f.parent.display(),
                            //         );
                            //         ui.checkbox(&mut f.marked, display_string);
                            //     }
                            // });
                        }
                    });
                }
            });

            ui.separator();
        });

        ctx.request_repaint();
    }
}

fn map_to(sets: &HashMap<Md5, DuplicateSet>) -> BTreeMap<PathBuf, DupsView> {
    // for (md5, set) in sets.iter() {
    //     for a in set.duplicates.values().flatten() {
    //         println!("md5 {}: set duplicates {:?}", md5, a.path);
    //     }
    // }

    let mut res: BTreeMap<PathBuf, DupsView> = BTreeMap::new();
    for (_, set) in sets.iter() {
        let first_occurrence = &set.first_occurrence.parent;

        for x in set.duplicates.values().flatten() {
            let p = &x.parent;

            let entry = DupsViewEntry {
                path: x.path.to_string_lossy().to_string(),
                marked_for_deletion: true,
            };

            if !res.contains_key(p) {
                let duplicates = vec![entry];
                let a = DupsView {
                    original_file: set.first_occurrence.path.clone(),
                    original_path: first_occurrence.clone(),
                    duplicates,
                    marked_for_deletion: true,
                };
                res.insert(p.clone(), a);
            } else {
                let dups = res.get_mut(p).unwrap();
                dups.duplicates.push(entry);
            }
        }
    }

    // println!("final MAP");
    // res.iter().for_each(|(k, v)| {
    //     for e in v.duplicates.iter() {
    //         println!("    {:?} --> {:?}", k, e);
    //     }
    // });

    res
}

/// ======================
/// MAIN
/// ======================
fn main() -> eframe::Result<()> {
    let app = DupeApp::default();

    eframe::run_native(
        "Duplicate Finder",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(app))),
    )
}

fn green_text(s: &str) -> String {
    format!("\x1b[32m{}\x1b[0m", s)
}

fn red_text(s: &str) -> String {
    format!("\x1b[31m{}\x1b[0m", s)
}

fn blue_text(s: &str) -> String {
    format!("\x1b[34m{}\x1b[0m", s)
}
