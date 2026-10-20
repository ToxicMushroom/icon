use crate::theme::DirectoryRef;
use crate::{IconFile, Icons, Theme};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

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
///
/// A cache can also be pre populated, and then shared (e.g. between threads) to look up icons
/// without touching the filesystem:
///
/// ```
/// use icon::{Icons, IconsCache};
///
/// let mut cache: IconsCache = Icons::new().into();
/// cache.pre_populate_cache();
///
/// let cache = &cache;
/// cache.find_cached_icon("firefox", 128, 1, "Adwaita");
/// ```
pub struct IconsCache {
    /// The [`Icons`] this cache was created from.
    icons: Icons,
    /// Mirrors `icons.themes`, but with all caches.
    /// The implementation should make sure that all keys present in `icons.themes`,
    /// also appear in this map. For the same reason, both `icons` and `themes` aren't `pub`;
    /// otherwise users could break that invariant.
    themes: HashMap<OsString, ThemeCache>,
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
        
        if let Some(icon) = theme.find_icon_here(icon_name, size, scale) {
            return Some(icon);
        }

        // or find it in the cache of one of its parents
        let theme = Arc::clone(&theme.theme);
        Self::parent_names(&theme)
            .find_map(|name| {
                self.themes
                    .get_mut(name)?
                    .find_icon_here(icon_name, size, scale)
            })
            .or_else(|| self.find_standalone_icon(icon_name))
    }

    /// Like [`find_cached_icon`](Self::find_cached_icon), with `theme` set to `"hicolor"`.
    /// 
    /// Note: hicolor is the fallback icon theme, into which third party applications install their icons
    /// ref https://specifications.freedesktop.org/icon-theme/latest/#directory_layout
    pub fn find_hicolor_cached_icon(
        &self,
        icon_name: &str,
        size: u32,
        scale: u32,
    ) -> Option<IconFile> {
        self.find_cached_icon(icon_name, size, scale, "hicolor")
    }

    /// Look up an icon by name, size, scale and theme.
    ///
    /// Unlike [`find_icon`](Self::find_icon), this never touches the filesystem nor modifies the
    /// cache, so it only needs shared access.
    pub fn find_cached_icon(
        &self,
        icon_name: &str,
        size: u32,
        scale: u32,
        theme: &str,
    ) -> Option<IconFile> {
        if icon_name.is_empty() {
            return None;
        }

        let theme = self.lookup_theme_cache(theme)?;

        theme
            .find_cached_icon_here(icon_name, size, scale)
            .or_else(|| {
                // or find it in the cache of one of its parents
                Self::parent_names(&theme.theme).find_map(|name| {
                    self.themes
                        .get(name)?
                        .find_cached_icon_here(icon_name, size, scale)
                })
            })
            .or_else(|| self.find_standalone_icon(icon_name))
    }

    /// The cache of the theme icons are looked up in first, given the name of the requested theme:
    /// that theme's, or `"hicolor"`'s if no theme by that name exists.
    fn lookup_theme_cache(&self, theme_name: &str) -> Option<&ThemeCache> {
        match self.theme_cache(theme_name) {
            Some(theme) => Some(theme),
            None => self.theme_cache("hicolor"),
        }
    }

    /// The (internal) names of the themes an icon is looked up in after `theme` itself, in order
    /// (see [`Theme::inherits_from`]), as in [`Theme::find_icon`].
    fn parent_names(theme: &Theme) -> impl Iterator<Item = &OsStr> {
        theme
            .inherits_from
            .iter()
            .map(|parent| parent.info.internal_name.as_os_str())
    }

    /// Using [`Icons::find_all_icons`], populate the cache with all icons available.
    ///
    /// As finding all icons may be much faster than finding many icons separately,
    /// you may opt to use this function if your application expects to be loading (almost) all
    /// icons anyway.
    pub fn pre_populate_cache(&mut self) {
        for theme in self.themes.values_mut() {
            theme.pre_populate_cache();
        }
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

        best_match(&self.theme, icon_files, size, scale).cloned()
    }

    /// Find an icon in this theme only, using **only** what is already cached.
    ///
    /// Once the cache holds every file of `icon_name` (e.g. after
    /// [`IconsCache::pre_populate_cache`], or a previous [`find_icon_here`](Self::find_icon_here)
    /// for it), this should find the same icon as [`find_icon_here`](Self::find_icon_here) does.
    pub fn find_cached_icon_here(&self, icon_name: &str, size: u32, scale: u32) -> Option<IconFile> {
        let icon_files = self.cache.get(icon_name)?;

        best_match(&self.theme, icon_files, size, scale).cloned()
    }

    /// Populate the cache with all icons in this theme (but not its parents).
    ///
    /// The files of each icon are cached in the order [`find_icon_files`](Theme::find_icon_files)
    /// would find them in: by directory, then base directory, then preferred file type.
    fn pre_populate_cache(&mut self) {
        let theme = &self.theme;
        let mut icons = Vec::new();

        for (dir_ref, dir) in theme.info.index.directories.iter().enumerate() {
            for base_dir in &theme.info.base_dirs {
                icons.extend(Theme::icon_files_in_directory_index(base_dir, dir));
                // a directory may hold an icon in several file types; lookups prefer some of them
                icons.sort_by_key(|icon| icon.file_type().preference());

                for icon in icons.drain(..) {
                    self.cache
                        .entry(icon.icon_name().into())
                        .or_default()
                        .push((dir_ref, icon));
                }
            }
        }
    }

    /// Empties the internal cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

/// Returns the IconFile that matches `size` and `scale` exactly, or else
/// the closest one.
///
/// This mirrors the matching in [`Theme::find_icon_here`].
fn best_match<'a>(
    theme: &Theme,
    icon_files: &'a [(DirectoryRef, IconFile)],
    size: u32,
    scale: u32,
) -> Option<&'a IconFile> {
    let directories = &theme.info.index.directories;

    // find an exact match:
    for (dir, icon) in icon_files {
        if directories[*dir].matches_size(size, scale) {
            return Some(icon);
        }
    }

    // else, find the closest match:
    // TODO(performance): can early return when min-distance == 0
    icon_files
        .iter()
        .min_by_key(|(dir, _)| directories[*dir].size_distance(size, scale))
        .map(|(_, icon)| icon)
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
    use std::ffi::OsString;
    use crate::FileType;
    use crate::cache::{IconsCache, ThemeCache};
    use crate::search::test::test_search;

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
        assert!(icons.themes.iter().all(|(_, c)| c.cache.len() == 0), "no icons in cache");

        icons.pre_populate_cache();

        assert_eq!(icons.themes.len(), 2, "test themes in cache");
        assert_eq!(icons.themes[&OsString::from("TestTheme")].cache.len(), 2);
        assert_eq!(icons.themes[&OsString::from("OtherTheme")].cache.len(), 1);
    }

    #[test]
    fn test_cached_lookup_matches_lookup() {
        let icons = test_search().search().icons();
        let mut cache = test_search().search().icons_cached();
        cache.pre_populate_cache();

        let queries = [
            // exact and closest size matches
            ("happy", 16, "TestTheme"),
            ("happy", 32, "TestTheme"),
            ("happy", 100, "TestTheme"),
            ("beautiful sunset", 64, "TestTheme"),
            // only in OtherTheme, which TestTheme inherits from
            ("pixel", 1, "TestTheme"),
            ("pixel", 1, "OtherTheme"),
            // an unknown theme falls back to hicolor, which doesn't exist here
            ("happy", 16, "NoSuchTheme"),
            // doesn't exist
            ("sad", 16, "TestTheme"),
            ("", 16, "TestTheme"),
        ];
        for (name, size, theme) in queries {
            assert_eq!(
                cache.find_cached_icon(name, size, 1, theme),
                icons.find_icon(name, size, 1, theme),
                "{name} at {size}px in {theme}"
            );
        }
        assert!(cache.find_cached_icon("pixel", 1, 1, "TestTheme").is_some());
    }

    #[test]
    fn test_cached_lookup_only_uses_the_cache() {
        let icons = test_search().search().icons();
        let theme = icons.theme("TestTheme").unwrap();
        let mut theme_cache: ThemeCache = theme.into();

        assert_eq!(theme_cache.find_cached_icon_here("happy", 16, 1), None, "not cached yet");
        assert!(theme_cache.cache.is_empty(), "the cache is left untouched");

        let icon = theme_cache.find_icon_here("happy", 16, 1);
        assert!(icon.is_some());
        assert_eq!(theme_cache.find_cached_icon_here("happy", 16, 1), icon, "cached now");
    }

    #[test]
    fn test_pre_population_prefers_file_types_like_lookups() {
        // "beautiful sunset" exists as both a .png and an .xpm, in the same directory
        let icons = test_search().search().icons();
        let mut cache = test_search().search().icons_cached();
        cache.pre_populate_cache();

        let icon = icons.find_icon("beautiful sunset", 64, 1, "TestTheme").unwrap();
        assert_eq!(icon.file_type(), FileType::Png);
        assert_eq!(
            cache.find_cached_icon("beautiful sunset", 64, 1, "TestTheme"),
            Some(icon.clone())
        );
        assert_eq!(
            cache.find_icon("beautiful sunset", 64, 1, "TestTheme"),
            Some(icon)
        );
    }

    #[test]
    fn test_lookup_uses_the_caches_of_parents() {
        // "pixel" only exists in OtherTheme, which TestTheme inherits from
        let icons = test_search().search().icons();
        let mut cache = test_search().search().icons_cached();

        let icon = cache.find_icon("pixel", 1, 1, "TestTheme");
        assert!(icon.is_some());
        assert_eq!(icon, icons.find_icon("pixel", 1, 1, "TestTheme"));

        let other_theme = cache.theme_cache("OtherTheme").unwrap();
        assert!(other_theme.cache.contains_key("pixel"), "the parent's cache is used");
        let test_theme = cache.theme_cache("TestTheme").unwrap();
        assert!(
            test_theme.cache.get("pixel").is_none_or(Vec::is_empty),
            "TestTheme has no files for pixel"
        );
    }
}
