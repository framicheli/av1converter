use std::fs;
use std::path::PathBuf;

/// `FileExplorer` maintains the current directory, a sorted list of entries,
/// and the currently selected index. It supports navigating directories,
/// moving the selection, and selecting files.
#[derive(Clone, Debug)]
pub struct FileExplorer {
    /// Current directory being explored.
    pub current_dir: PathBuf,
    /// Sorted list of files and folders in `current_dir`.
    pub files: Vec<PathBuf>,
    /// Index of the currently selected item.
    pub selected_index: usize,
}

impl Default for FileExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl FileExplorer {
    /// Creates a new `FileExplorer` starting at the process working directory.
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut explorer = Self {
            current_dir,
            files: Vec::new(),
            selected_index: 0,
        };
        explorer.load_directory();
        explorer
    }

    /// Loads the contents of `current_dir` into `files`.
    ///
    /// Directories are listed first, followed by files. If the directory
    /// has a parent, a virtual `..` entry is added to allow navigating upward.
    pub fn load_directory(&mut self) {
        self.files.clear();
        self.selected_index = 0;

        // Add ".." for going up a directory
        if self.current_dir.parent().is_some() {
            self.files.push(self.current_dir.join(".."));
        }

        if let Ok(entries) = fs::read_dir(&self.current_dir) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    files.push(path);
                }
            }

            dirs.sort();
            files.sort();

            self.files.append(&mut dirs);
            self.files.append(&mut files);
        }
    }

    /// Moves the selection to the next entry.
    pub fn next(&mut self) {
        if !self.files.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.files.len();
        }
    }

    /// Moves the selection to the previous entry.
    pub fn previous(&mut self) {
        if !self.files.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.files.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    /// Selects the current entry.
    ///
    /// - If it is a directory, enters that directory.
    /// - If it is `..`, moves to the parent directory.
    /// - If it is a file, returns its path.
    pub fn select(&mut self) -> Option<PathBuf> {
        if self.files.is_empty() {
            return None;
        }

        let selected = self.files[self.selected_index].clone();

        if selected.ends_with("..") {
            if let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                self.load_directory();
            }
        } else if selected.is_dir() {
            self.current_dir = selected;
            self.load_directory();
        } else {
            return Some(selected);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs::{File, create_dir};

    fn setup_temp_fs() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut base = std::env::temp_dir();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        base.push(format!("pdm_file_explorer_test_{}", unique));

        fs::create_dir_all(&base).unwrap();
        fs::create_dir(base.join("folder")).unwrap();
        File::create(base.join("file.txt")).unwrap();

        base
    }

    #[test]
    fn loads_directory_entries() {
        let dir = setup_temp_fs();
        let mut explorer = FileExplorer {
            current_dir: dir,
            files: vec![],
            selected_index: 0,
        };

        explorer.load_directory();
        assert!(explorer.files.len() >= 2);
    }

    #[test]
    fn next_and_previous_wrap() {
        let dir = setup_temp_fs();
        let mut explorer = FileExplorer {
            current_dir: dir,
            files: vec![PathBuf::from("a"), PathBuf::from("b")],
            selected_index: 0,
        };

        explorer.next();
        assert_eq!(explorer.selected_index, 1);

        explorer.next();
        assert_eq!(explorer.selected_index, 0);

        explorer.previous();
        assert_eq!(explorer.selected_index, 1);
    }

    #[test]
    fn selecting_file_returns_path() {
        let dir = setup_temp_fs();
        let file = dir.join("file.txt");

        let mut explorer = FileExplorer {
            current_dir: dir,
            files: vec![file.clone()],
            selected_index: 0,
        };

        let result = explorer.select();
        assert_eq!(result, Some(file));
    }

    #[test]
    fn selecting_parent_directory_moves_up() {
        let base = setup_temp_fs();
        let child = base.join("child");
        fs::create_dir(&child).unwrap();

        let mut explorer = FileExplorer {
            current_dir: child.clone(),
            files: vec![],
            selected_index: 0,
        };

        explorer.load_directory();

        // First entry must be ".."
        assert!(explorer.files[0].ends_with(".."));

        // Select the ".." entry
        let result = explorer.select();

        // It should move to parent and not return a file
        assert!(result.is_none());
        assert_eq!(explorer.current_dir, base);
        assert!(!explorer.files.is_empty());
    }

    #[test]
    fn selecting_directory_enters_directory() {
        let base = setup_temp_fs();
        let folder = base.join("folder");

        let mut explorer = FileExplorer {
            current_dir: base.clone(),
            files: vec![folder.clone()],
            selected_index: 0,
        };

        let result = explorer.select();
        assert!(result.is_none());
        assert_eq!(explorer.current_dir, folder);
    }

    #[test]
    fn default_constructs_explorer() {
        let explorer = FileExplorer::default();
        assert!(!explorer.current_dir.as_os_str().is_empty());
    }

    #[test]
    fn previous_decrements_when_not_zero() {
        let dir = setup_temp_fs();
        let mut explorer = FileExplorer {
            current_dir: dir,
            files: vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")],
            selected_index: 2,
        };

        explorer.previous();
        assert_eq!(explorer.selected_index, 1);
    }
}
