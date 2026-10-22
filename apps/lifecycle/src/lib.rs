//! Public-API consumer exercising windows, entity ownership, and subscriptions.

use gpui::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, KeyBinding, Menu, MenuItem, Render,
    Subscription, Window, WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::*, px,
    rgb, size,
};

actions!(lifecycle, [NewWindow, Checkpoint, CloseWindow, Quit]);

/// Shared entity owned by the application callbacks and each live view.
#[derive(Default)]
pub struct Session {
    opened: usize,
    closed: usize,
    released: usize,
    checkpoints: usize,
}

struct CheckpointReached;
impl EventEmitter<CheckpointReached> for Session {}

/// A window owns its view; subscription callbacks must not keep that view alive.
pub struct LifecycleView {
    session: Entity<Session>,
    focus: FocusHandle,
    observed_changes: usize,
    received_events: usize,
    subscriptions: Vec<Subscription>,
}

impl LifecycleView {
    fn new(session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&session, |view, _, cx| {
            view.observed_changes += 1;
            cx.notify();
        });
        let events = cx.subscribe(&session, |view, _, _: &CheckpointReached, cx| {
            view.received_events += 1;
            cx.notify();
        });
        let release_session = session.clone();
        let release = cx.on_release(move |_, cx| {
            release_session.update(cx, |session, cx| {
                session.released += 1;
                cx.notify();
            });
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            session,
            focus,
            observed_changes: 0,
            received_events: 0,
            subscriptions: vec![observation, events, release],
        }
    }

    fn checkpoint(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.checkpoints += 1;
            cx.emit(CheckpointReached);
            cx.notify();
        });
    }
}

impl Render for LifecycleView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Checkpoint, _, cx| view.checkpoint(cx)))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(rgb(0x20242b))
            .text_color(rgb(0xe8edf2))
            .child("Lifecycle API exercise")
            .child(format!("Window {}", window.window_handle().window_id().as_u64()))
            .child(format!("Opened: {} | Closed: {} | Released views: {}", session.opened, session.closed, session.released))
            .child(format!("Shared checkpoints: {}", session.checkpoints))
            .child(format!("This view: {} notifications | {} events | {} retained subscriptions", self.observed_changes, self.received_events, self.subscriptions.len()))
            .child(format!("{}-N: new window", modifier()))
            .child(format!("{}-Shift-U: shared checkpoint", modifier()))
            .child(format!("{}-W: close window | {}-Q: quit", modifier(), modifier()))
            .child("Open two windows, checkpoint in either, then close one. The survivor should show one closed window and one released view.")
    }
}

fn modifier() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// Register the exercise's public action, keyboard, and close-observation paths.
pub fn configure(cx: &mut App) -> Entity<Session> {
    let session = cx.new(|_| Session::default());
    let closed_session = session.clone();
    cx.on_window_closed(move |cx, _| {
        closed_session.update(cx, |session, cx| {
            session.closed += 1;
            cx.notify();
        });
    })
    .detach();
    cx.bind_keys([
        KeyBinding::new(&format!("{}-n", modifier()), NewWindow, None),
        KeyBinding::new(&format!("{}-shift-u", modifier()), Checkpoint, None),
        KeyBinding::new(&format!("{}-w", modifier()), CloseWindow, None),
        KeyBinding::new(&format!("{}-q", modifier()), Quit, None),
    ]);
    let new_window_session = session.clone();
    cx.on_action(move |_: &NewWindow, cx| {
        if let Err(error) = open_window(&new_window_session, cx) {
            eprintln!("Could not open another lifecycle window: {error:#}");
        }
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![Menu::new("Lifecycle").items(vec![
        MenuItem::action("New Window", NewWindow),
        MenuItem::action("Shared Checkpoint", Checkpoint),
        MenuItem::action("Close Window", CloseWindow),
        MenuItem::separator(),
        MenuItem::action("Quit", Quit),
    ])]);
    session
}

/// Open the same view through `App::open_window` in the native app and mock tests.
pub fn open_window(
    session: &Entity<Session>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<LifecycleView>> {
    let bounds = Bounds::centered(None, size(px(720.0), px(480.0)), cx);
    let view_session = session.clone();
    let window = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| LifecycleView::new(view_session, window, cx)),
    )?;
    session.update(cx, |session, cx| {
        session.opened += 1;
        cx.notify();
    });
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn checkpoint_actions_reach_both_windows(cx: &mut TestAppContext) {
        let session = cx.update(configure);
        let first = cx.update(|cx| open_window(&session, cx)).unwrap();
        let second = cx.update(|cx| open_window(&session, cx)).unwrap();
        cx.run_until_parked();
        let before = second
            .read_with(cx, |view, _| view.observed_changes)
            .unwrap();
        cx.dispatch_action(first.into(), Checkpoint);
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(session.read(cx).checkpoints, 1);
            for window in [first, second] {
                let view = window.read(cx).unwrap();
                assert_eq!(view.received_events, 1);
            }
            assert!(second.read(cx).unwrap().observed_changes > before);
        });
    }

    #[gpui::test]
    fn closing_a_window_releases_its_view_and_keeps_the_survivor(cx: &mut TestAppContext) {
        let session = cx.update(configure);
        let first = cx.update(|cx| open_window(&session, cx)).unwrap();
        let second = cx.update(|cx| open_window(&session, cx)).unwrap();
        let weak = first.root(cx).unwrap().downgrade();
        cx.run_until_parked();
        cx.dispatch_action(first.into(), CloseWindow);
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 1);
        assert!(weak.upgrade().is_none());
        cx.read(|cx| {
            assert_eq!(session.read(cx).closed, 1);
            assert_eq!(session.read(cx).released, 1);
            assert!(second.read(cx).is_ok());
        });
        cx.dispatch_action(second.into(), Checkpoint);
        assert_eq!(
            second
                .read_with(cx, |view, _| view.received_events)
                .unwrap(),
            1
        );
    }

    #[gpui::test]
    fn an_external_entity_handle_delays_release_after_window_closure(cx: &mut TestAppContext) {
        let session = cx.update(configure);
        let window = cx.update(|cx| open_window(&session, cx)).unwrap();
        let retained = window.root(cx).unwrap();
        let weak = retained.downgrade();
        cx.run_until_parked();
        cx.dispatch_action(window.into(), CloseWindow);
        cx.run_until_parked();
        assert!(cx.windows().is_empty());
        assert!(weak.upgrade().is_some());
        cx.read(|cx| assert_eq!(session.read(cx).released, 0));
        // GPUI processes entity release effects at the application update boundary.
        cx.update(|_| drop(retained));
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
        cx.read(|cx| assert_eq!(session.read(cx).released, 1));
    }

    #[gpui::test]
    fn dropping_subscriptions_stops_callbacks_without_affecting_other_views(
        cx: &mut TestAppContext,
    ) {
        let session = cx.update(configure);
        let first = cx.update(|cx| open_window(&session, cx)).unwrap();
        let second = cx.update(|cx| open_window(&session, cx)).unwrap();
        cx.run_until_parked();
        first
            .update(cx, |view, _, _| {
                view.subscriptions.drain(..2).for_each(drop)
            })
            .unwrap();
        let before = first
            .read_with(cx, |view, _| view.observed_changes)
            .unwrap();
        cx.dispatch_action(second.into(), Checkpoint);
        cx.read(|cx| {
            let view = first.read(cx).unwrap();
            assert_eq!(view.received_events, 0);
            assert_eq!(view.observed_changes, before);
            assert_eq!(second.read(cx).unwrap().received_events, 1);
        });
    }

    #[gpui::test]
    fn key_bindings_dispatch_checkpoint_and_new_window(cx: &mut TestAppContext) {
        let session = cx.update(configure);
        let first = cx.update(|cx| open_window(&session, cx)).unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(first.into(), &format!("{}-shift-u", modifier()));
        cx.read(|cx| assert_eq!(session.read(cx).checkpoints, 1));
        cx.simulate_keystrokes(first.into(), &format!("{}-n", modifier()));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 2);
        cx.read(|cx| assert_eq!(session.read(cx).opened, 2));
    }
}
