use crate::theme::DirectoryRef;
use crate::{IconFile, Icons, Theme};
use futures::executor::ThreadPool;
use futures::future::{join, join_all};
use futures::task::SpawnExt;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;
use std::time::Instant;

/// Caching version of [`Icons`].
///
/// # Example
///
/// ```
/// use icon::{Icons, IconsCache};
///
/// let mut cache: IconsCache = Icons::new().into();
/// cache.find_icon("firefox", 128, 1, "Adwaita");
/// // Subsequent queries for any size/scale for "firefox" will utilize the cache.
/// ```
pub struct IconsCache {
    /// The [`Icons`] this cache was created from.
    icons: Icons,
    /// Mirrors `icons.themes`, but with all caches.
    /// The implementation should make sure that all keys present in `icons.themes`,
    /// also appear in this map. For the same reason, both `icons` and `themes` aren't `pub`;
    /// otherwise users could break that invariant.
    themes: ahash::AHashMap<OsString, ThemeCache>,
}

impl IconsCache {
    /// Creates a new [`IconsCache`] from [`Icons`].
    pub fn from_icons(icons: Icons) -> Self {
        icons.into()
    }

    /// Like [`find_icon`](self.find_icon), with `theme` being `"hicolor"`, which is the default icon theme.
    ///
    /// Caching version of [`Icons::find_default_icon`]
    pub fn find_default_icon(
        &mut self,
        icon_name: &str,
        size: u32,
        scale: u32,
    ) -> Option<IconFile> {
        self.find_icon(icon_name, size, scale, "hicolor")
    }

    /// Look up an icon by name, size, scale and theme.
    ///
    /// Caching version of [`Icons::find_icon`]. For more details on how icon matching works,
    /// check out the documentation of [`Icons::find_icon`].
    pub fn find_icon(
        &mut self,
        icon_name: &str,
        size: u32,
        scale: u32,
        theme: &str,
    ) -> Option<IconFile> {
        if icon_name.is_empty() {
            return None;
        }

        let theme = match self.theme_cache_mut(theme) {
            Some(theme) => theme,
            None => self.theme_cache_mut("hicolor")?,
        };

        theme
            .find_icon(icon_name, size, scale)
            .or_else(|| self.find_standalone_icon(icon_name))
    }

    /// Using [`Icons::find_all_icons`], populate the cache with all icons available.
    ///
    /// As finding all icons may be much faster than finding many icons separately,
    /// you may opt to use this function if your application expects to be loading (almost) all
    /// icons anyway.
    pub async fn pre_populate_cache(&mut self) {
        let pool = ThreadPool::new().expect("failed to create thread pool");
        let start = Instant::now();
        let mut futures = vec![];

        for theme in self.icons.themes.values() {
            let theme = theme.clone();
            let theme2 = theme.clone();

            let theme_name = theme.info.internal_name.as_os_str();
            let theme_name2 = OsString::from(theme_name);
            let theme_cache = self.themes.remove(theme_name);

            let Some(mut theme_cache) = theme_cache else {
                #[cfg(feature = "log")]
                log::warn!("skipping theme without a cache entry, this shouldn't ever happen!");
                continue;
            };

            let (tx, rx) = crossbeam_channel::bounded::<(DirectoryRef, IconFile)>(1000);
            let collection_thread = async {
                for (dir_ref, icon) in rx {
                    theme_cache
                        .cache
                        .entry(icon.icon_name().into())
                        .or_insert_with(Default::default)
                        .push((dir_ref, icon));
                }
                theme_cache
            };
            let handle = pool
                .spawn_with_handle(collection_thread)
                .expect("failed to spawn thread");

            // Completes before collection_thread.
            let task2 = pool
                .spawn_with_handle(async move {
                    for (dir_idx, dir) in theme2.info.index.directories.iter().enumerate() {
                        // Each "dir" may map to multiple actual fs directories if the theme
                        // has multiple base_dirs.
                        for icon in theme2
                            .info
                            .base_dirs
                            .iter()
                            .map(|base_dir| base_dir.join(&dir.directory_name))
                            .flat_map(|dir| dir.read_dir()) // Skip directories we can't read.
                            .flatten() // Flatten out the dir iterator,
                            .flatten() // and skip Err entries.
                            .flat_map(|dir_entry| IconFile::from_path_buf(dir_entry.path()))
                        {
                            // And then skip all files that aren't icons.
                            tx.send((dir_idx, icon)).expect("failed to send icon");
                        }
                    }
                })
                .expect("that we can spawn thread");

            futures.push(async move {
                let (theme, _) = join(handle, task2).await;
                (theme, theme_name2)
            });
        }

        for (theme_cache, theme_name) in join_all(futures).await {
            self.themes.insert(theme_name, theme_cache);
        }

        let time_taken = Instant::now() - start;
        println!("Loop body: {time_taken:?}");
    }

    /// Access a known icon theme cache by name.
    ///
    /// Analogous to [`Icons::theme`].
    pub fn theme_cache(&self, theme_name: &str) -> Option<&ThemeCache> {
        let theme_name: &OsStr = theme_name.as_ref();
        self.themes.get(theme_name)
    }

    /// Access, mutably, a known icon theme cache by name.
    pub fn theme_cache_mut(&mut self, theme_name: &str) -> Option<&mut ThemeCache> {
        let theme_name: &OsStr = theme_name.as_ref();
        self.themes.get_mut(theme_name)
    }

    /// Look up a standalone icon by name.
    ///
    /// Cache version of [`Icons::find_standalone_icon`].
    pub fn find_standalone_icon(&self, icon_name: &str) -> Option<IconFile> {
        self.icons.find_standalone_icon(icon_name)
    }

    /// Access the [`Icons`] this cache uses.
    pub fn icons(&self) -> &Icons {
        &self.icons
    }
}

impl From<Icons> for IconsCache {
    fn from(icons: Icons) -> Self {
        let themes = icons
            .themes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();

        Self { icons, themes }
    }
}

/// Caching version of [`Theme`].
pub struct ThemeCache {
    theme: Arc<Theme>,
    // Cache of icon names to a list of files and the ref (index) of the directory they're in.
    cache: ahash::AHashMap<smol_str::SmolStr, Vec<(DirectoryRef, IconFile)>>,
}

impl ThemeCache {
    /// Create a new [`ThemeCache`] from a given [`Theme`].
    pub fn from_theme(theme: Arc<Theme>) -> Self {
        theme.into()
    }

    /// Find an icon in this theme or any of its dependencies, utilizing and populating the internal
    /// cache where possible.
    ///
    /// Analogous to [Theme::find_icon].
    pub fn find_icon(&mut self, icon_name: &str, size: u32, scale: u32) -> Option<IconFile> {
        self.find_icon_here(icon_name, size, scale).or_else(|| {
            // or find it in one of our parents
            self.theme
                .inherits_from
                .iter()
                .find_map(|theme| theme.find_icon_here(icon_name, size, scale))
        })
    }

    /// Find an icon in this theme only, utilizing and populating the internal cache where possible.
    ///
    /// This function is analogous to [`Theme::find_icon_here`].
    // for people editing this function: make sure to check, and keep in sync, the behaviour of
    // Theme::find_icon_here with this function.
    pub fn find_icon_here(&mut self, icon_name: &str, size: u32, scale: u32) -> Option<IconFile> {
        // If `icon_name` isn't in the cache yet,
        // let's start by finding all(!) of its files; this is more expensive than the normal
        // lookup function, but we pay the cost upfront to make subsequent lookups quicker!

        let icon_files: &Vec<_> = self
            .cache
            .entry(icon_name.into())
            // if this icon isn't in the cache already, find its files and insert those:
            .or_insert_with(|| self.theme.find_icon_files(icon_name).collect());

        // find an exact match:
        for (dir, ico) in icon_files {
            let dir = &self.theme.info.index.directories[*dir];

            if dir.matches_size(size, scale) {
                return Some(ico.clone());
            }
        }

        // else, find the closest match:
        // TODO(performance): can early return when min-distance == 0
        let icon = icon_files.iter().min_by_key(|(dir, _)| {
            let dir = &self.theme.info.index.directories[*dir];

            dir.size_distance(size, scale)
        });

        icon.map(|(_, ico)| ico.clone())
    }

    /// Empties the internal cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl From<Arc<Theme>> for ThemeCache {
    fn from(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            cache: Default::default(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::IconSearch;
    use crate::cache::{IconsCache, ThemeCache};
    use crate::search::test::test_search;
    use futures::executor::block_on;
    use std::ffi::OsString;
    use std::time::Instant;

    #[test]
    fn test_icons_cached() {
        let icons = test_search().search().icons();
        let icon_original = icons.find_icon("happy", 16, 1, "TestTheme").unwrap();
        let mut icons_cache: IconsCache = icons.into();
        let icon_cached = icons_cache.find_icon("happy", 16, 1, "TestTheme").unwrap();

        assert_eq!(icon_original, icon_cached);
    }

    #[test]
    fn test_cached_entry_persists() {
        let icons = test_search().search().icons();
        let theme = icons.theme("TestTheme").unwrap();

        let icon_original = theme.find_icon_here("happy", 16, 1).unwrap();

        let mut theme_cache: ThemeCache = theme.into();

        assert!(theme_cache.cache.is_empty(), "cache is not yet populated");

        let icon = theme_cache.find_icon_here("happy", 16, 1).unwrap();
        assert_eq!(icon.icon_name(), "happy");
        println!("{:?}", icon);

        assert!(
            theme_cache.cache.contains_key("happy"),
            "cache contains happy icon"
        );

        let icon_cached = theme_cache.find_icon_here("happy", 16, 1).unwrap();

        assert_eq!(
            icon, icon_cached,
            "cached icon is the same as the first one"
        );
        assert_eq!(
            icon_original, icon,
            "cached icon is the same as the original"
        );
    }

    #[test]
    fn test_pre_population() {
        let mut icons = test_search().search().icons_cached();

        assert_eq!(icons.themes.len(), 2, "test themes in cache");
        assert!(
            icons.themes.iter().all(|(_, c)| c.cache.len() == 0),
            "no icons in cache"
        );

        icons.pre_populate_cache();

        assert_eq!(icons.themes.len(), 2, "test themes in cache");
        assert_eq!(icons.themes[&OsString::from("TestTheme")].cache.len(), 2);
        assert_eq!(icons.themes[&OsString::from("OtherTheme")].cache.len(), 1);
    }

    #[test]
    fn test_real_pre_population() {
        let start = Instant::now();
        let mut icons = IconSearch::new().search().icons_cached();
        let time_taken = Instant::now() - start;
        println!("Theme init: {time_taken:?}");
        let start = Instant::now();
        block_on(std::hint::black_box(icons.pre_populate_cache()));
        let time_taken = Instant::now() - start;
        println!("Pre-popuplate cache: {time_taken:?}")
    }
}
