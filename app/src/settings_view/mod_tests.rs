use settings_page::{FilteredPageType, MatchData, PageType, SettingsWidget, search_terms_match};
use warpui::elements::Empty;
use warpui::{App, AppContext, Element, Entity, View};

use super::*;
use crate::appearance::Appearance;

// ── SettingsSection classification ──────────────────────────────────────────

// LOCAL FORK: the "Agents" umbrella (Warp Agent / Profiles / MCP servers /
// Knowledge / Third party CLI agents) went with the agent, so the tests that
// asserted `is_ai_subpage`, `ai_subpages()` and the AI backing-page mapping are
// gone. The subpage/umbrella machinery they shared with the Code and Cloud
// platform groups is still live, and is exercised below through those groups.

#[test]
fn code_subpages_are_identified() {
    assert!(SettingsSection::CodeIndexing.is_code_subpage());
    assert!(SettingsSection::EditorAndCodeReview.is_code_subpage());

    assert!(!SettingsSection::Code.is_code_subpage());
    assert!(!SettingsSection::CloudEnvironments.is_code_subpage());
}

#[test]
fn cloud_platform_subpages_are_identified() {
    assert!(SettingsSection::CloudEnvironments.is_cloud_platform_subpage());
    assert!(SettingsSection::OzCloudAPIKeys.is_cloud_platform_subpage());

    assert!(!SettingsSection::Account.is_cloud_platform_subpage());
    assert!(!SettingsSection::CodeIndexing.is_cloud_platform_subpage());
}

#[test]
fn is_subpage_covers_all_umbrella_types() {
    // All subpages under any umbrella should return true.
    for section in SettingsSection::code_subpages()
        .iter()
        .chain(SettingsSection::cloud_platform_subpages())
    {
        assert!(section.is_subpage(), "{section:?} should be a subpage");
    }
    assert!(SettingsSection::CodeIndexing.is_subpage());
    assert!(SettingsSection::EditorAndCodeReview.is_subpage());
    assert!(SettingsSection::CloudEnvironments.is_subpage());
    assert!(SettingsSection::OzCloudAPIKeys.is_subpage());

    // Top-level pages should not be subpages.
    assert!(!SettingsSection::Account.is_subpage());
    assert!(!SettingsSection::Code.is_subpage());
    assert!(!SettingsSection::Privacy.is_subpage());
}

// ── parent_page_section mapping ─────────────────────────────────────────────

#[test]
fn code_subpages_map_to_code_backing_page() {
    assert_eq!(
        SettingsSection::CodeIndexing.parent_page_section(),
        SettingsSection::Code
    );
    assert_eq!(
        SettingsSection::EditorAndCodeReview.parent_page_section(),
        SettingsSection::Code
    );
}

#[test]
fn cloud_platform_subpages_map_to_their_backing_pages() {
    assert_eq!(
        SettingsSection::CloudEnvironments.parent_page_section(),
        SettingsSection::CloudEnvironments
    );
    assert_eq!(
        SettingsSection::OzCloudAPIKeys.parent_page_section(),
        SettingsSection::OzCloudAPIKeys
    );
}

#[test]
fn non_subpage_sections_map_to_themselves() {
    assert_eq!(
        SettingsSection::Account.parent_page_section(),
        SettingsSection::Account
    );
    assert_eq!(
        SettingsSection::MCPServers.parent_page_section(),
        SettingsSection::MCPServers
    );
    assert_eq!(
        SettingsSection::Privacy.parent_page_section(),
        SettingsSection::Privacy
    );
}

// ── subpage lists ───────────────────────────────────────────────────────────

#[test]
fn subpage_lists_contain_only_their_own_subpages() {
    let code = SettingsSection::code_subpages();
    assert!(code.contains(&SettingsSection::CodeIndexing));
    assert!(code.contains(&SettingsSection::EditorAndCodeReview));
    assert!(!code.contains(&SettingsSection::Code));
    assert!(!code.contains(&SettingsSection::CloudEnvironments));

    let cloud = SettingsSection::cloud_platform_subpages();
    assert!(cloud.contains(&SettingsSection::CloudEnvironments));
    assert!(cloud.contains(&SettingsSection::OzCloudAPIKeys));
    assert!(!cloud.contains(&SettingsSection::Account));
    assert!(!cloud.contains(&SettingsSection::CodeIndexing));
}

// ── MatchData behavior ──────────────────────────────────────────────────────

#[test]
fn match_data_uncounted_true_is_truthy() {
    assert!(MatchData::Uncounted(true).is_truthy());
}

#[test]
fn match_data_uncounted_false_is_not_truthy() {
    assert!(!MatchData::Uncounted(false).is_truthy());
}

#[test]
fn match_data_countable_nonzero_is_truthy() {
    assert!(MatchData::Countable(3).is_truthy());
    assert!(MatchData::Countable(1).is_truthy());
}

#[test]
fn match_data_countable_zero_is_not_truthy() {
    assert!(!MatchData::Countable(0).is_truthy());
}

// ── Display / FromStr round-trip ────────────────────────────────────────────

#[test]
fn subpage_display_names_are_correct() {
    assert_eq!(
        SettingsSection::CodeIndexing.to_string(),
        "Indexing and projects"
    );
    assert_eq!(
        SettingsSection::EditorAndCodeReview.to_string(),
        "Editor and Code Review"
    );
    assert_eq!(
        SettingsSection::CloudEnvironments.to_string(),
        "Environments"
    );
    assert_eq!(
        SettingsSection::OzCloudAPIKeys.to_string(),
        "Oz Cloud API Keys"
    );
}

#[test]
fn subpage_from_str_parses_display_names() {
    // Deep links (`warp://settings?page=<name>`) and persisted section strings
    // are parsed through FromStr, so the display names must keep round-tripping.
    assert_eq!(
        SettingsSection::from_str("Indexing and projects"),
        Ok(SettingsSection::CodeIndexing)
    );
    assert_eq!(
        SettingsSection::from_str("Editor and Code Review"),
        Ok(SettingsSection::EditorAndCodeReview)
    );
    assert_eq!(
        SettingsSection::from_str("Oz Cloud API Keys"),
        Ok(SettingsSection::OzCloudAPIKeys)
    );
}

// ── Subpage search filter simulation ────────────────────────────────────────
// These tests simulate the per-subpage search filtering logic used in
// handle_search_editor_event: each subpage should only be visible if its
// own widgets' search terms match, not if a sibling subpage's terms match.

/// Helper: given a map of subpage→MatchData, returns which subpages are visible.
fn visible_subpages(
    subpage_filter: &HashMap<SettingsSection, MatchData>,
    subpages: &[SettingsSection],
) -> Vec<SettingsSection> {
    subpages
        .iter()
        .filter(|s| {
            subpage_filter
                .get(s)
                .map(|md| md.is_truthy())
                .unwrap_or(false)
        })
        .copied()
        .collect()
}

#[test]
fn search_matching_one_subpage_shows_only_that_subpage() {
    // Simulate: a search matched the "Editor and Code Review" subpage's widgets
    // but not its sibling's.
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(1),
    );

    let visible = visible_subpages(&filter, SettingsSection::code_subpages());

    assert_eq!(visible, vec![SettingsSection::EditorAndCodeReview]);
}

#[test]
fn search_matching_several_subpages_shows_each_of_them() {
    // A term present in more than one subpage's widgets keeps every matching
    // subpage visible while still hiding the non-matching ones.
    let all_subpages: Vec<SettingsSection> = SettingsSection::code_subpages()
        .iter()
        .chain(SettingsSection::cloud_platform_subpages())
        .copied()
        .collect();

    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(2));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(1),
    );
    filter.insert(SettingsSection::CloudEnvironments, MatchData::Countable(0));
    filter.insert(SettingsSection::OzCloudAPIKeys, MatchData::Countable(0));

    let visible = visible_subpages(&filter, &all_subpages);

    assert!(visible.contains(&SettingsSection::CodeIndexing));
    assert!(visible.contains(&SettingsSection::EditorAndCodeReview));
    assert!(!visible.contains(&SettingsSection::CloudEnvironments));
    assert!(!visible.contains(&SettingsSection::OzCloudAPIKeys));
}

#[test]
fn empty_search_shows_no_subpages_in_filter() {
    // When search is cleared, subpage_filter is empty — all subpages fall back
    // to their backing page visibility (Uncounted(true) by default).
    let filter: HashMap<SettingsSection, MatchData> = HashMap::new();

    let visible = visible_subpages(&filter, SettingsSection::code_subpages());

    // No entries in filter means no subpage-specific filtering; all return false
    // from the filter map. The actual rendering code falls back to the backing
    // page's pages_filter which defaults to Uncounted(true).
    assert!(visible.is_empty());
}

#[test]
fn search_with_no_matches_hides_all_subpages() {
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(0),
    );

    let visible = visible_subpages(&filter, SettingsSection::code_subpages());

    assert!(visible.is_empty());
}

/// Helper: check if an umbrella should be visible given a subpage filter.
fn umbrella_visible(
    subpage_filter: &HashMap<SettingsSection, MatchData>,
    umbrella_subpages: &[SettingsSection],
) -> bool {
    umbrella_subpages.iter().any(|s| {
        subpage_filter
            .get(s)
            .map(|md| md.is_truthy())
            .unwrap_or(false)
    })
}

#[test]
fn umbrella_hidden_when_no_subpages_match() {
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(0),
    );

    assert!(!umbrella_visible(&filter, SettingsSection::code_subpages()));
}

// ── cycle_pages search filter ────────────────────────────────────────────────
// These tests validate the logic added to cycle_pages() to ensure arrow key
// navigation respects the active search filter.

/// Mirrors the filter predicate used in cycle_pages() when search is active.
fn section_passes_nav_filter(
    section: SettingsSection,
    subpage_filter: &HashMap<SettingsSection, MatchData>,
    pages_filter: &[(SettingsSection, MatchData)],
) -> bool {
    if let Some(md) = subpage_filter.get(&section) {
        md.is_truthy()
    } else {
        let backing = section.parent_page_section();
        pages_filter
            .iter()
            .any(|(s, md)| *s == backing && md.is_truthy())
    }
}

#[test]
fn nav_filter_includes_matching_subpage_and_excludes_others() {
    let mut subpage_filter = HashMap::new();
    subpage_filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    subpage_filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(1),
    );
    subpage_filter.insert(SettingsSection::CloudEnvironments, MatchData::Countable(0));
    subpage_filter.insert(SettingsSection::OzCloudAPIKeys, MatchData::Countable(0));

    // No page-level filter entries needed since every subpage above has a
    // subpage_filter entry.
    let pages_filter: Vec<(SettingsSection, MatchData)> = vec![];

    assert!(!section_passes_nav_filter(
        SettingsSection::CodeIndexing,
        &subpage_filter,
        &pages_filter
    ));
    assert!(section_passes_nav_filter(
        SettingsSection::EditorAndCodeReview,
        &subpage_filter,
        &pages_filter
    ));
    assert!(!section_passes_nav_filter(
        SettingsSection::CloudEnvironments,
        &subpage_filter,
        &pages_filter
    ));
    assert!(!section_passes_nav_filter(
        SettingsSection::OzCloudAPIKeys,
        &subpage_filter,
        &pages_filter
    ));
}

#[test]
fn nav_filter_falls_back_to_pages_filter_for_top_level_pages() {
    // Top-level pages (Account, Appearance, etc.) have no subpage_filter entry.
    // They fall back to pages_filter using parent_page_section() == themselves.
    let subpage_filter: HashMap<SettingsSection, MatchData> = HashMap::new();
    let pages_filter = vec![
        (SettingsSection::Account, MatchData::Uncounted(true)),
        (SettingsSection::Appearance, MatchData::Countable(0)),
        (SettingsSection::Features, MatchData::Uncounted(true)),
    ];

    assert!(section_passes_nav_filter(
        SettingsSection::Account,
        &subpage_filter,
        &pages_filter
    ));
    assert!(!section_passes_nav_filter(
        SettingsSection::Appearance,
        &subpage_filter,
        &pages_filter
    ));
    assert!(section_passes_nav_filter(
        SettingsSection::Features,
        &subpage_filter,
        &pages_filter
    ));
}

#[test]
fn umbrella_visible_when_any_subpage_matches() {
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(1),
    );

    assert!(umbrella_visible(&filter, SettingsSection::code_subpages()));
}

// ── Search auto-select simulation ───────────────────────────────────────────
// These tests simulate the auto-select logic in handle_search_editor_event:
// when the current subpage is filtered out by search, the view should jump
// to the first visible subpage or page.

/// Simulates the "is current still visible" check from the search handler.
/// Returns true if `current` is still visible given the subpage_filter and
/// a list of (backing_section, is_truthy) pairs for pages_filter.
fn is_current_visible(
    current: SettingsSection,
    subpage_filter: &HashMap<SettingsSection, MatchData>,
    pages_visible: &[(SettingsSection, bool)],
) -> bool {
    if let Some(md) = subpage_filter.get(&current) {
        return md.is_truthy();
    }
    let backing = current.parent_page_section();
    pages_visible
        .iter()
        .any(|(section, visible)| *section == backing && *visible)
}

/// Simulates finding the first visible section from the nav_items order.
fn first_visible_section(
    nav_order: &[SettingsSection],
    subpage_filter: &HashMap<SettingsSection, MatchData>,
    pages_visible: &[(SettingsSection, bool)],
) -> Option<SettingsSection> {
    nav_order.iter().copied().find(|section| {
        if let Some(md) = subpage_filter.get(section) {
            md.is_truthy()
        } else {
            let backing = section.parent_page_section();
            pages_visible
                .iter()
                .any(|(s, visible)| *s == backing && *visible)
        }
    })
}

#[test]
fn auto_select_jumps_away_from_filtered_out_subpage() {
    // User is on CodeIndexing and searches a term that only the sibling subpage
    // matches.
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(2),
    );

    let current = SettingsSection::CodeIndexing;
    assert!(
        !is_current_visible(current, &filter, &[]),
        "CodeIndexing should not be visible when it has 0 matches"
    );

    let nav_order = SettingsSection::code_subpages();
    let first = first_visible_section(nav_order, &filter, &[]);
    assert_eq!(
        first,
        Some(SettingsSection::EditorAndCodeReview),
        "Should auto-select the first visible subpage"
    );
}

#[test]
fn auto_select_stays_on_current_when_it_matches() {
    // User is on CodeIndexing and searches a term CodeIndexing matches.
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(1));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(0),
    );

    let current = SettingsSection::CodeIndexing;
    assert!(
        is_current_visible(current, &filter, &[]),
        "CodeIndexing should remain visible when it has matches"
    );
}

#[test]
fn auto_select_falls_back_to_top_level_page_when_no_subpages_match() {
    // All Code subpages filtered out, but Account (top-level) is still visible.
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(0),
    );

    let pages_visible = vec![
        (SettingsSection::Account, true),
        (SettingsSection::Code, false),
    ];

    // Nav order includes top-level Account before the Code subpages.
    let nav_order = vec![
        SettingsSection::Account,
        SettingsSection::CodeIndexing,
        SettingsSection::EditorAndCodeReview,
    ];

    let first = first_visible_section(&nav_order, &filter, &pages_visible);
    assert_eq!(
        first,
        Some(SettingsSection::Account),
        "Should fall back to Account when no subpages match"
    );
}

#[test]
fn auto_select_handles_standalone_subpage_via_backing_page() {
    // CloudEnvironments is a subpage that is its own backing page, so it is not
    // in subpage_filter. It should be visible if that backing page is visible.
    let filter = HashMap::new(); // no per-subpage entries for CloudEnvironments

    let pages_visible = vec![
        (SettingsSection::CloudEnvironments, true),
        (SettingsSection::Code, false),
    ];

    let current = SettingsSection::CloudEnvironments;
    assert!(
        is_current_visible(current, &filter, &pages_visible),
        "CloudEnvironments should be visible via its own backing page"
    );
}

#[test]
fn auto_select_with_no_matches_anywhere() {
    let mut filter = HashMap::new();
    filter.insert(SettingsSection::CodeIndexing, MatchData::Countable(0));
    filter.insert(
        SettingsSection::EditorAndCodeReview,
        MatchData::Countable(0),
    );

    let pages_visible = vec![
        (SettingsSection::Account, false),
        (SettingsSection::Code, false),
    ];

    let nav_order = vec![
        SettingsSection::Account,
        SettingsSection::CodeIndexing,
        SettingsSection::EditorAndCodeReview,
    ];

    let first = first_visible_section(&nav_order, &filter, &pages_visible);
    assert_eq!(
        first, None,
        "No section should be selected when nothing matches"
    );
}

// ── Collapsed umbrella nav-stop behavior ────────────────────────────────────
// Verify that arrow-key navigation lands on a collapsed umbrella as a single
// stop (and activates it by jumping to the first subpage, which auto-expands
// the umbrella) instead of silently skipping over it.

use nav::{SettingsNavItem, SettingsUmbrella};

/// Builds a nav-items layout in the shape `SettingsView::new` produces: a mix
/// of top-level pages and collapsible umbrellas, with two umbrellas adjacent so
/// cycling between them is covered.
///
/// LOCAL FORK: the sidebar no longer builds any umbrella (the "Agents" group
/// went with the agent, and the "Code" / "Cloud platform" groups were flattened
/// away), but `build_nav_stops` / `current_stop_index` still implement umbrella
/// cycling and these tests are the only coverage of it. The fixture therefore
/// groups the surviving Code and Cloud platform subpage sections.
fn realistic_nav_items() -> Vec<SettingsNavItem> {
    vec![
        SettingsNavItem::Page(SettingsSection::Account),
        SettingsNavItem::Umbrella(SettingsUmbrella::new(
            "Code",
            SettingsSection::code_subpages().to_vec(),
        )),
        SettingsNavItem::Umbrella(SettingsUmbrella::new(
            "Cloud platform",
            SettingsSection::cloud_platform_subpages().to_vec(),
        )),
        SettingsNavItem::Page(SettingsSection::BillingAndUsage),
        SettingsNavItem::Page(SettingsSection::Teams),
    ]
}

/// Mutably flips an umbrella's `expanded` flag at `nav_index`.
fn set_expanded(nav_items: &mut [SettingsNavItem], nav_index: usize, expanded: bool) {
    if let Some(SettingsNavItem::Umbrella(u)) = nav_items.get_mut(nav_index) {
        u.expanded = expanded;
    } else {
        panic!("nav_items[{nav_index}] is not an Umbrella");
    }
}

#[test]
fn collapsed_umbrella_is_a_single_nav_stop() {
    let nav_items = realistic_nav_items();
    // All umbrellas default to collapsed.
    let stops = build_nav_stops(&nav_items, |_| true);

    // Expect: Account, <Code umbrella>, <Cloud platform umbrella>,
    // BillingAndUsage, Teams.
    assert_eq!(stops.len(), 5);
    assert!(matches!(
        stops[0],
        NavStop::Section(SettingsSection::Account)
    ));
    assert!(matches!(
        stops[1],
        NavStop::CollapsedUmbrella {
            nav_index: 1,
            first_subpage: SettingsSection::CodeIndexing,
            last_subpage: SettingsSection::EditorAndCodeReview,
        }
    ));
    assert!(matches!(
        stops[2],
        NavStop::CollapsedUmbrella {
            nav_index: 2,
            first_subpage: SettingsSection::CloudEnvironments,
            last_subpage: SettingsSection::OzCloudAPIKeys,
        }
    ));
    assert!(matches!(
        stops[3],
        NavStop::Section(SettingsSection::BillingAndUsage)
    ));
    assert!(matches!(stops[4], NavStop::Section(SettingsSection::Teams)));
}

#[test]
fn expanded_umbrella_produces_section_stop_per_subpage() {
    let mut nav_items = realistic_nav_items();
    // Expand the Code umbrella so each of its subpages becomes a nav stop.
    set_expanded(&mut nav_items, 1, true);

    let stops = build_nav_stops(&nav_items, |_| true);

    // Expect: Account, CodeIndexing, EditorAndCodeReview,
    // <Cloud platform umbrella>, BillingAndUsage, Teams.
    let sections: Vec<_> = stops
        .iter()
        .map(|s| match s {
            NavStop::Section(section) => format!("{section:?}"),
            NavStop::CollapsedUmbrella { nav_index, .. } => format!("Umbrella@{nav_index}"),
        })
        .collect();
    assert_eq!(
        sections,
        vec![
            "Account",
            "CodeIndexing",
            "EditorAndCodeReview",
            "Umbrella@2",
            "BillingAndUsage",
            "Teams",
        ]
    );
}

#[test]
fn collapsed_umbrella_with_filtered_subpages_uses_first_visible_subpage() {
    // When a search filter hides the first subpage, activating the collapsed
    // umbrella should land on the *next* visible subpage (still auto-expanding).
    let nav_items = realistic_nav_items();

    let stops = build_nav_stops(&nav_items, |section| {
        // Hide CodeIndexing (first Code subpage); keep the rest.
        section != SettingsSection::CodeIndexing
    });

    let code_stop = stops
        .iter()
        .find(|s| matches!(s, NavStop::CollapsedUmbrella { nav_index: 1, .. }))
        .expect("Code umbrella should still be a collapsed stop");

    match code_stop {
        NavStop::CollapsedUmbrella {
            first_subpage,
            last_subpage,
            ..
        } => {
            assert_eq!(
                *first_subpage,
                SettingsSection::EditorAndCodeReview,
                "CodeIndexing is hidden by the filter, so the first visible subpage is EditorAndCodeReview"
            );
            assert_eq!(
                *last_subpage,
                SettingsSection::EditorAndCodeReview,
                "the last visible subpage is also EditorAndCodeReview once CodeIndexing is hidden"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn umbrella_with_no_visible_subpages_is_skipped_entirely() {
    let nav_items = realistic_nav_items();

    let stops = build_nav_stops(&nav_items, |section| !section.is_code_subpage());

    // The Code umbrella's subpages are all Code subpages, so the entire
    // umbrella should be absent from the nav order.
    assert!(
        stops
            .iter()
            .all(|s| !matches!(s, NavStop::CollapsedUmbrella { nav_index: 1, .. })),
        "Code umbrella should not appear when none of its subpages are visible"
    );
    // The still-visible Cloud platform umbrella remains as a stop.
    assert!(
        stops
            .iter()
            .any(|s| matches!(s, NavStop::CollapsedUmbrella { nav_index: 2, .. }))
    );
}

#[test]
fn filtered_out_top_level_page_is_skipped() {
    let nav_items = realistic_nav_items();

    let stops = build_nav_stops(&nav_items, |section| section != SettingsSection::Teams);

    assert!(
        !stops
            .iter()
            .any(|s| matches!(s, NavStop::Section(SettingsSection::Teams))),
        "Teams should be filtered out entirely"
    );
    // But other pages remain.
    assert!(
        stops
            .iter()
            .any(|s| matches!(s, NavStop::Section(SettingsSection::Account)))
    );
}

// ── current_stop_index ──────────────────────────────────────────────────────

#[test]
fn current_stop_index_matches_section_stop() {
    let nav_items = realistic_nav_items();
    let stops = build_nav_stops(&nav_items, |_| true);

    let idx = current_stop_index(&stops, &nav_items, SettingsSection::BillingAndUsage);
    assert_eq!(idx, Some(3));
}

#[test]
fn current_stop_index_maps_subpage_to_collapsed_umbrella() {
    // Edge case: the user manually collapsed the Code umbrella while still on
    // one of its subpages. The collapsed umbrella should match as the current
    // stop so arrow-key cycling continues from the umbrella's position.
    let nav_items = realistic_nav_items();
    let stops = build_nav_stops(&nav_items, |_| true);

    let idx = current_stop_index(&stops, &nav_items, SettingsSection::EditorAndCodeReview);
    assert_eq!(
        idx,
        Some(1),
        "EditorAndCodeReview is under the collapsed Code umbrella at nav_index 1"
    );
}

#[test]
fn current_stop_index_returns_none_when_section_is_not_present() {
    let nav_items = realistic_nav_items();
    // Filter out all Code subpages (and therefore the Code umbrella) entirely.
    let stops = build_nav_stops(&nav_items, |section| !section.is_code_subpage());

    // EditorAndCodeReview isn't directly in stops, and no remaining collapsed
    // umbrella contains it, so current_stop_index should return None.
    assert_eq!(
        current_stop_index(&stops, &nav_items, SettingsSection::EditorAndCodeReview),
        None
    );
}

// ── next_stop_index wrapping ────────────────────────────────────────────────

#[test]
fn next_stop_index_wraps_at_ends() {
    assert_eq!(next_stop_index(0, 3, CycleDirection::Up), 2);
    assert_eq!(next_stop_index(2, 3, CycleDirection::Down), 0);
    assert_eq!(next_stop_index(1, 3, CycleDirection::Up), 0);
    assert_eq!(next_stop_index(1, 3, CycleDirection::Down), 2);
}

#[test]
fn next_stop_index_handles_single_stop() {
    assert_eq!(next_stop_index(0, 1, CycleDirection::Up), 0);
    assert_eq!(next_stop_index(0, 1, CycleDirection::Down), 0);
}

// ── End-to-end cycling (no search) ──────────────────────────────────────────
// These tests simulate the sequence of nav-stop activations that would result
// from repeatedly pressing Down/Up, ensuring a collapsed umbrella is never
// skipped over.

/// Computes the section that would become active after applying the direction
/// once, starting from `current`. Mirrors the final target-resolution step in
/// `cycle_pages`.
fn simulate_cycle(
    nav_items: &[SettingsNavItem],
    stops: &[NavStop],
    current: SettingsSection,
    direction: CycleDirection,
) -> SettingsSection {
    let active = current_stop_index(stops, nav_items, current)
        .expect("current should exist in stops in these tests");
    let next = next_stop_index(active, stops.len(), direction);
    match stops[next] {
        NavStop::Section(section) => section,
        NavStop::CollapsedUmbrella {
            first_subpage,
            last_subpage,
            ..
        } => match direction {
            CycleDirection::Up => last_subpage,
            CycleDirection::Down => first_subpage,
        },
    }
}

#[test]
fn arrow_down_from_page_into_collapsed_umbrella_lands_on_first_subpage() {
    let nav_items = realistic_nav_items();
    let stops = build_nav_stops(&nav_items, |_| true);

    // Pressing Down from Account should auto-expand Code and select
    // CodeIndexing, not skip over to the next top-level page.
    let next = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::Account,
        CycleDirection::Down,
    );
    assert_eq!(next, SettingsSection::CodeIndexing);
}

#[test]
fn arrow_up_from_page_into_collapsed_umbrella_lands_on_last_subpage() {
    let nav_items = realistic_nav_items();
    let stops = build_nav_stops(&nav_items, |_| true);

    // Pressing Up from BillingAndUsage should land on the collapsed Cloud
    // platform umbrella, which resolves to OzCloudAPIKeys (last visible
    // subpage) so the user continues moving in natural reading order rather
    // than being jumped back to the top of the umbrella.
    let next = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::BillingAndUsage,
        CycleDirection::Up,
    );
    assert_eq!(next, SettingsSection::OzCloudAPIKeys);
}

#[test]
fn arrow_up_into_collapsed_umbrella_respects_search_filter_for_last_subpage() {
    let nav_items = realistic_nav_items();
    // Hide the last Cloud platform subpage; the last *visible* subpage of the
    // still-collapsed Cloud platform umbrella should be CloudEnvironments.
    let is_visible = |section: SettingsSection| section != SettingsSection::OzCloudAPIKeys;
    let stops = build_nav_stops(&nav_items, is_visible);

    // From BillingAndUsage, Up should land on the last *visible* Cloud platform
    // subpage (CloudEnvironments), not on the filtered-out OzCloudAPIKeys.
    let next = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::BillingAndUsage,
        CycleDirection::Up,
    );
    assert_eq!(next, SettingsSection::CloudEnvironments);
}

#[test]
fn arrow_down_from_expanded_last_subpage_leaves_umbrella() {
    let mut nav_items = realistic_nav_items();
    set_expanded(&mut nav_items, 2, true); // expand Cloud platform
    let stops = build_nav_stops(&nav_items, |_| true);

    // OzCloudAPIKeys is the last Cloud platform subpage; Down should move to
    // BillingAndUsage (the next top-level page in the nav order).
    let next = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::OzCloudAPIKeys,
        CycleDirection::Down,
    );
    assert_eq!(next, SettingsSection::BillingAndUsage);
}

#[test]
fn arrow_down_across_adjacent_collapsed_umbrellas() {
    let nav_items = realistic_nav_items();
    // Both Code and Cloud platform umbrellas are collapsed.
    let stops = build_nav_stops(&nav_items, |_| true);

    // From Account, Down should land on the first Code subpage (Code umbrella
    // auto-expands).
    let next_after_account = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::Account,
        CycleDirection::Down,
    );
    assert_eq!(next_after_account, SettingsSection::CodeIndexing);

    // From the Code umbrella stop (i.e. the user is "on" CodeIndexing which
    // maps back to the collapsed umbrella), pressing Down again should land
    // on the Cloud platform umbrella's first subpage.
    let next_after_code = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::CodeIndexing,
        CycleDirection::Down,
    );
    assert_eq!(next_after_code, SettingsSection::CloudEnvironments);
}

#[test]
fn arrow_down_collapsed_umbrella_respects_search_filter() {
    let nav_items = realistic_nav_items();
    // Search filter hides CodeIndexing so the first visible Code subpage is
    // EditorAndCodeReview.
    let is_visible = |section: SettingsSection| section != SettingsSection::CodeIndexing;
    let stops = build_nav_stops(&nav_items, is_visible);

    // From Account, Down should land on EditorAndCodeReview (first visible
    // subpage of the still-collapsed Code umbrella), not on CodeIndexing.
    let next = simulate_cycle(
        &nav_items,
        &stops,
        SettingsSection::Account,
        CycleDirection::Down,
    );
    assert_eq!(next, SettingsSection::EditorAndCodeReview);
}
// ── Active subpage filter reapply after rebuild (APP-4922) ───────────────────
// Searching on an AI/Code subpage rebuilds the subpage's PageType (via
// set_active_subpage), which resets its widget filter to every widget; the
// active query must be reapplied so only matching widgets render. These tests
// exercise the real PageType::Uncategorized filter lifecycle and the real
// search_terms_match predicate. The production reapply call sites in mod.rs
// (handle_search_editor_event/cycle_pages/SelectAndRefresh) need a full
// ViewContext<SettingsView>, so they are verified via computer-use screenshots.

/// Minimal View so PageType<V> can be instantiated in a unit test without the
/// full SettingsView/ViewContext the production reapply call sites require.
struct TestSettingsView;

impl Entity for TestSettingsView {
    type Event = ();
}

impl View for TestSettingsView {
    fn ui_name() -> &'static str {
        "TestSettingsView"
    }

    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

/// A SettingsWidget whose only test-relevant state is its search terms; render
/// is never invoked by the filter lifecycle under test.
struct StubWidget {
    terms: &'static str,
}

impl SettingsWidget for StubWidget {
    type View = TestSettingsView;

    fn search_terms(&self) -> &str {
        self.terms
    }

    fn render(&self, _: &Self::View, _: &Appearance, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

/// A fresh Uncategorized page mirroring set_active_subpage -> build_page ->
/// new_uncategorized: every widget index visible by default.
fn stub_widgets_page() -> PageType<TestSettingsView> {
    let widgets: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> = vec![
        Box::new(StubWidget {
            terms: "warp agent global ai toggle",
        }),
        Box::new(StubWidget {
            terms: "active ai autosuggestions prompt",
        }),
        Box::new(StubWidget {
            terms: "ai input model api key",
        }),
        Box::new(StubWidget {
            terms: "file search fuzzy opener",
        }),
        Box::new(StubWidget {
            terms: "voice input",
        }),
    ];
    PageType::new_uncategorized(widgets, None)
}

/// Number of widgets the page would render under its current filter.
fn visible_widget_count<V: View>(page: &PageType<V>) -> usize {
    let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
        panic!("expected Uncategorized page");
    };
    widgets.len()
}

#[test]
fn search_terms_match_direct_unit_checks() {
    // Empty query matches everything (mirrors PageType::update_filter's guard).
    assert!(search_terms_match("warp agent global ai toggle", ""));
    // All-words, case-insensitive, non-contiguous.
    assert!(search_terms_match(
        "active ai autosuggestions prompt",
        "autosuggestions"
    ));
    assert!(search_terms_match(
        "active ai autosuggestions prompt",
        "ACTIVE AI"
    ));
    assert!(search_terms_match(
        "file search fuzzy opener",
        "file search"
    ));
    // Every word must appear.
    assert!(!search_terms_match(
        "warp agent global ai toggle",
        "file search"
    ));
    assert!(!search_terms_match(
        "active ai autosuggestions prompt",
        "autosuggestions key"
    ));
}

#[test]
fn rebuild_resets_filter_to_all_widgets() {
    // Searching "file search" matches exactly one widget. A freshly built page
    // (mirroring set_active_subpage -> build_page -> new_uncategorized) resets
    // the filter to every widget, so without reapplying update_filter the
    // subpage would show all widgets.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            let md = page.update_filter("file search", ctx);
            assert!(md.is_truthy());
            assert_eq!(visible_widget_count(&page), 1);

            let rebuilt = stub_widgets_page();
            assert_eq!(
                visible_widget_count(&rebuilt),
                5,
                "rebuild resets the filter to all widgets when update_filter isn't reapplied"
            );
        });
    });
}

#[test]
fn rebuild_with_reapply_keeps_only_matching_widgets() {
    // The fix: after a rebuild, reapply update_filter with the active query so
    // only matching widgets render on the restored subpage.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("file search", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("file search", ctx);
            assert_eq!(
                visible_widget_count(&rebuilt),
                1,
                "reapplying the filter after a rebuild keeps only matching widgets visible"
            );
        });
    });
}

#[test]
fn reapply_handles_multi_word_and_case() {
    // A multi-word, case-insensitive query survives the rebuild + reapply cycle.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("AI INPUT", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("AI INPUT", ctx);
            assert_eq!(visible_widget_count(&rebuilt), 1);
        });
    });
}

#[test]
fn empty_query_after_reapply_shows_all_widgets() {
    // When the search is cleared, the subpage shows all widgets again.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("agent", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("", ctx);
            assert_eq!(
                visible_widget_count(&rebuilt),
                5,
                "an empty query restores every widget on the subpage"
            );
        });
    });
}
