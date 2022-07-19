use std::path::{Path, PathBuf};
use crate::exclusions::Exclusions;
use crate::filter::Filter;
use std::fs::metadata;
use tokio::fs::metadata as tokio_metadata;
use std::collections::VecDeque;
use async_recursion::async_recursion;


struct FileExplorer {
    // list of directories/files to scan
    base_paths: Vec<PathBuf>,
    // ignore any directories/files found in the exclusions
    exclusions: Exclusions,
    // filter files types
    filter: Filter,
    // pending walk directories
    walk_dirs: VecDeque<PathBuf>,
    // pending walk files
    walk_files: VecDeque<PathBuf>,
    // accumulation of all paths which failed scan
    failed_paths: Vec<(PathBuf, std::io::Error)>,
    //TODO (guyt): base path Option<Path> (19/07/2022)
}

//TODO (guyt): consider if I might want to check if base_paths collide (19/07/2022)

impl FileExplorer {
    fn new(base_paths: Vec<PathBuf>, exclusions: Exclusions, filter: Filter) -> std::io::Result<Self> {
        let mut dirs = VecDeque::new();
        let mut files = VecDeque::new();
        for base_path in base_paths.iter() {
            match Self::is_dir(base_path) {
                Ok(res) => {
                    if res {
                        dirs.push_front(base_path.to_owned())
                    } else {
                        files.push_front(base_path.to_owned())
                    }
                }
                Err(err) => {
                    println!("Failed adding path to scan. Path: {:?}. Error: {}", base_path, err);
                }
            }
        }


        Ok(Self {
            base_paths,
            exclusions,
            filter,
            walk_dirs: dirs,
            walk_files: files,
            failed_paths: vec![],
        })
    }

    fn is_dir(path: &Path) -> std::io::Result<bool> {
        let md = metadata(path)?;
        Ok(md.is_dir())
    }

    async fn async_is_dir(path: &Path) -> std::io::Result<bool> {
        let md = tokio_metadata(path).await?;
        Ok(md.is_dir())
    }

    #[async_recursion]
    async fn next(&mut self) -> Option<PathBuf> {
        if let Some(next_file) = self.walk_files.pop_back() {
            return Some(next_file);
        }

        let next_dir = self.walk_dirs.pop_back()?;
        let mut entries = match tokio::fs::read_dir(&next_dir).await {
            Ok(entries) => entries,
            Err(err) => {
                println!("Failed reading dir with error {} for dir {:?}", err, next_dir);
                self.failed_paths.push((next_dir, err));
                return self.next().await;
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            match Self::async_is_dir(&entry.path()).await {
                Ok(is_dir) => {
                    if is_dir {
                        self.walk_dirs.push_back(entry.path())
                    } else {
                        self.walk_files.push_back(entry.path())
                    }
                }
                Err(err) => {
                    println!("Failed reading entry with error {} for path {:?}", err, entry.path());
                    self.failed_paths.push((entry.path(), err))
                }
            }
        }
        self.next().await
    }
}

#[cfg(test)]
mod tests {
    use super::FileExplorer;
    use pretty_assertions::{assert_eq};
    use std::path::{Path, PathBuf};
    use rand::distributions::{Alphanumeric, DistString};
    use tokio::fs::File;
    use tempdir::TempDir;
    use crate::exclusions::Exclusions;
    use crate::filter::Filter;

    #[tokio::test]
    async fn test_iterate_files_in_dir() {
        let dir = new_dir();
        let number_of_files = 5;
        create_files_in_dir(dir.path(), number_of_files).await;
        let mut explorer = FileExplorer::new(vec![dir.path().to_owned()], Exclusions {}, Filter {}).unwrap();

        let mut counter = 0;
        while let Some(_) = explorer.next().await {
            counter += 1;
        }
        assert_eq!(counter, number_of_files);
    }


    #[tokio::test]
    async fn test_iterate_multiple_dirs() {
        let first_dir = new_dir();
        let second_dir = new_dir();
        let number_of_files = 5;
        create_files_in_dir(first_dir.path(), number_of_files).await;
        create_files_in_dir(second_dir.path(), number_of_files).await;
        let mut explorer = FileExplorer::new(vec![first_dir.path().to_owned(), second_dir.path().to_owned()], Exclusions {}, Filter {}).unwrap();

        let mut counter = 0;
        while let Some(_) = explorer.next().await {
            counter += 1;
        }
        assert_eq!(counter, number_of_files * 2);
    }

    #[tokio::test]
    async fn test_iterate_inner_folder() {
        let outer_dir = new_dir();
        let outer_files = 3;
        create_files_in_dir(outer_dir.path(), outer_files).await;

        let inner_dir = TempDir::new_in(outer_dir.path(), "inner").unwrap();
        let inner_files = 5;
        create_files_in_dir(inner_dir.path(), inner_files).await;

        let mut explorer = FileExplorer::new(vec![outer_dir.path().to_owned()], Exclusions {}, Filter {}).unwrap();
        let mut counter = 0;
        while let Some(_) = explorer.next().await {
            counter += 1;
        }
        assert_eq!(counter, outer_files + inner_files);
    }

    #[tokio::test]
    async fn test_iterate_bad_files() {
        let outer_dir = new_dir();
        let outer_files = 3;
        create_files_in_dir(outer_dir.path(), outer_files).await;

        let inner_dir = TempDir::new_in(outer_dir.path(), "inner").unwrap();
        let inner_files = 5;
        create_files_in_dir(inner_dir.path(), inner_files).await;

        let mut explorer = FileExplorer::new(vec!["does not exist".into(), outer_dir.path().to_owned(), "does not exist".into()], Exclusions {}, Filter {}).unwrap();
        let mut counter = 0;
        while let Some(_) = explorer.next().await {
            counter += 1;
        }
        assert_eq!(counter, outer_files + inner_files);
    }

    async fn create_files_in_dir(dir: &Path, number_of_files: usize) {
        for _ in 0..number_of_files {
            File::create(create_rand_file(dir)).await.unwrap();
        }
    }

    fn rand_string() -> String {
        Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
    }

    fn create_rand_file(base: &Path) -> PathBuf {
        base.clone().join(rand_string())
    }

    fn new_dir() -> TempDir {
        TempDir::new(&*rand_string()).unwrap()
    }
}