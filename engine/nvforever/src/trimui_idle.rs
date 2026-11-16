//! Skip unchanged UI layout/stack work on the handheld. Changed inputs and removals
//! invalidate the cached geometry; the UI itself and all interaction remain enabled.
use bevy::prelude::*;
use bevy::text::{EmSize, RemSize};
use bevy::ui::{ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ContentSize, FixedNode, IgnoreScroll, LayoutConfig, UiSystems};

pub struct IdleUiPlugin;

impl Plugin for IdleUiPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("NVF_IDLE_UI").as_deref() == Ok("0") { return; }
        app.configure_sets(PostUpdate, (
            UiSystems::Layout.run_if(layout_dirty),
            UiSystems::Stack.after(UiSystems::Layout).run_if(layout_dirty),
        ));
        info!("trimui: unchanged UI layout and stack caching enabled (NVF_IDLE_UI=0 disables)");
    }
}

type LayoutChanges = Or<(
    Changed<Node>, Changed<ContentSize>, Changed<UiTransform>, Changed<EmSize>,
    Changed<ComputedUiRenderTargetInfo>, Changed<ComputedUiTargetCamera>, Changed<ScrollPosition>, Changed<Outline>,
    Changed<LayoutConfig>, Changed<IgnoreScroll>, Added<FixedNode>,
)>;

#[allow(clippy::type_complexity)]
fn layout_dirty(
    changed: Query<(), (With<Node>, LayoutChanges)>,
    hierarchy: Query<(), Or<(Changed<Children>, Changed<ChildOf>, Changed<GlobalZIndex>, Changed<ZIndex>)>>,
    rem: Res<RemSize>,
    mut removed: (
        RemovedComponents<Node>, RemovedComponents<Children>, RemovedComponents<ChildOf>,
        RemovedComponents<FixedNode>, RemovedComponents<ScrollPosition>, RemovedComponents<Outline>,
        RemovedComponents<LayoutConfig>, RemovedComponents<IgnoreScroll>,
        RemovedComponents<GlobalZIndex>, RemovedComponents<ZIndex>,
        RemovedComponents<ContentSize>, RemovedComponents<EmSize>, RemovedComponents<UiTransform>, RemovedComponents<ComputedUiRenderTargetInfo>,
    ),
    mut first: Local<bool>,
) -> bool {
    // Drain every reader even if another input already dirtied layout. Otherwise a
    // removal can remain queued and cause a spurious refresh on the next idle frame.
    let removals = removed.0.read().count() + removed.1.read().count() + removed.2.read().count()
        + removed.3.read().count() + removed.4.read().count() + removed.5.read().count()
        + removed.6.read().count() + removed.7.read().count() + removed.8.read().count() + removed.9.read().count()
        + removed.10.read().count() + removed.11.read().count() + removed.12.read().count() + removed.13.read().count();
    let initial = !*first;
    *first = true;
    initial || rem.is_changed() || removals > 0 || !changed.is_empty() || !hierarchy.is_empty()
}
