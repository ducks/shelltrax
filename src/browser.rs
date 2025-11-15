use crate::list::ListSelector;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum BrowserItem {
    UpDirectory,    // represents `..`
    Entry(PathBuf), // actual file or folder
}

pub struct BrowserState {
    pub current_dir: PathBuf,
    pub list: ListSelector<BrowserItem>,
}

impl BrowserState {
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let entries = read_dir_items(&current_dir);

        Self {
            current_dir,
            list: ListSelector::new(entries),
        }
    }

    pub fn open_selected(&mut self) {
        match self.list.selected_item() {
            Some(BrowserItem::UpDirectory) => self.go_up(),
            Some(BrowserItem::Entry(path)) if path.is_dir() => {
                self.current_dir = path.clone();
                let entries = read_dir_items(&self.current_dir);
                self.list.set_entries(entries);
            }
            _ => {}
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            let entries = read_dir_items(&self.current_dir);
            self.list.set_entries(entries);
        }
    }

    pub fn move_up(&mut self) {
        self.list.move_up();
    }

    pub fn move_down(&mut self) {
        self.list.move_down();
    }

    pub fn go_to_top(&mut self) {
        self.list.go_to_top();
    }

    pub fn go_to_bottom(&mut self) {
        self.list.go_to_bottom();
    }

    pub fn jump_to_match(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }

        let query_lower = query.to_lowercase();

        // Search through entries for a match
        for (i, item) in self.list.entries.iter().enumerate() {
            let matches = match item {
                BrowserItem::UpDirectory => false,
                BrowserItem::Entry(path) => {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        file_name.to_lowercase().contains(&query_lower)
                    } else {
                        false
                    }
                }
            };

            if matches {
                self.list.selected = i;
                return;
            }
        }
    }
}

fn read_dir_items(dir: &PathBuf) -> Vec<BrowserItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![BrowserItem::UpDirectory];
    };

    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| !s.starts_with('.'))
                .unwrap_or(false)
        })
        .map(BrowserItem::Entry)
        .collect();

    paths.sort_by_key(|item| match item {
        BrowserItem::Entry(path) => path.clone(),
        _ => PathBuf::new(),
    });

    let mut all = vec![BrowserItem::UpDirectory];
    all.extend(paths);
    all
}
