// Copyright 2020 the Xilem Authors and the Druid Authors
// SPDX-License-Identifier: Apache-2.0

//! The context types that are passed into various widget methods.

use std::time::Duration;

use accesskit::TreeUpdate;
use parley::{FontContext, LayoutContext};
use tracing::{trace, warn};
use vello::kurbo::Vec2;

use crate::action::Action;
use crate::passes::layout::run_layout_on;
use crate::render_root::{MutateCallback, RenderRootSignal, RenderRootState};
use crate::text::TextBrush;
use crate::tree_arena::{ArenaMutChildren, ArenaRefChildren};
use crate::widget::{WidgetMut, WidgetRef, WidgetState};
use crate::{AllowRawMut, BoxConstraints, Insets, Point, Rect, Size, Widget, WidgetId, WidgetPod};

// Note - Most methods defined in this file revolve around `WidgetState` fields.
// Consider reading `WidgetState` documentation (especially the documented naming scheme)
// before editing context method code.

/// A macro for implementing methods on multiple contexts.
///
/// There are a lot of methods defined on multiple contexts; this lets us only
/// have to write them out once.
macro_rules! impl_context_method {
    ($ty:ty,  { $($method:item)+ } ) => {
        impl $ty { $($method)+ }
    };
    ( $ty:ty, $($more:ty),+, { $($method:item)+ } ) => {
        impl_context_method!($ty, { $($method)+ });
        impl_context_method!($($more),+, { $($method)+ });
    };
}

/// A context provided inside of [`WidgetMut`].
///
/// When you declare a mutable reference type for your widget, methods of this type
/// will have access to a `MutateCtx`. If that method mutates the widget in a way that
/// requires a later pass (for instance, if your widget has a `set_color` method),
/// you will need to signal that change in the pass (eg `request_paint`).
///
// TODO add tutorial - See https://github.com/linebender/xilem/issues/376
pub struct MutateCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) parent_widget_state: Option<&'a mut WidgetState>,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
}

/// A context provided to methods of widgets requiring shared, read-only access.
#[derive(Clone, Copy)]
pub struct QueryCtx<'a> {
    pub(crate) global_state: &'a RenderRootState,
    pub(crate) widget_state: &'a WidgetState,
    pub(crate) widget_state_children: ArenaRefChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaRefChildren<'a, Box<dyn Widget>>,
}

/// A context provided to event handling methods of widgets.
///
/// Widgets should call [`request_paint`](Self::request_paint) whenever an event causes a change
/// in the widget's appearance, to schedule a repaint.
pub struct EventCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
    pub(crate) target: WidgetId,
    pub(crate) allow_pointer_capture: bool,
    pub(crate) is_handled: bool,
}

/// A context provided to the [`Widget::register_children`] method on widgets.
pub struct RegisterCtx<'a> {
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
    #[cfg(debug_assertions)]
    pub(crate) registered_ids: Vec<WidgetId>,
}

/// A context provided to the [`update`] method on widgets.
///
/// [`update`]: Widget::update
pub struct UpdateCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
}

/// A context provided to layout handling methods of widgets.
///
/// As of now, the main service provided is access to a factory for
/// creating text layout objects, which are likely to be useful
/// during widget layout.
pub struct LayoutCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
}

pub struct ComposeCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a mut WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
}

/// A context passed to paint methods of widgets.
pub struct PaintCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
    pub(crate) debug_paint: bool,
}

pub struct AccessCtx<'a> {
    pub(crate) global_state: &'a mut RenderRootState,
    pub(crate) widget_state: &'a WidgetState,
    pub(crate) widget_state_children: ArenaMutChildren<'a, WidgetState>,
    pub(crate) widget_children: ArenaMutChildren<'a, Box<dyn Widget>>,
    pub(crate) tree_update: &'a mut TreeUpdate,
    pub(crate) rebuild_all: bool,
    pub(crate) scale_factor: f64,
}

// --- MARK: GETTERS ---
// Methods for all context types
impl_context_method!(
    MutateCtx<'_>,
    QueryCtx<'_>,
    EventCtx<'_>,
    UpdateCtx<'_>,
    LayoutCtx<'_>,
    ComposeCtx<'_>,
    PaintCtx<'_>,
    AccessCtx<'_>,
    {
        /// get the `WidgetId` of the current widget.
        pub fn widget_id(&self) -> WidgetId {
            self.widget_state.id
        }

        #[allow(dead_code)]
        /// Helper method to get a direct reference to a child widget from its `WidgetPod`.
        fn get_child<Child: Widget>(&self, child: &'_ WidgetPod<Child>) -> &'_ Child {
            let child_ref = self
                .widget_children
                .get_child(child.id())
                .expect("get_child: child not found");
            child_ref.item.as_dyn_any().downcast_ref::<Child>().unwrap()
        }

        #[allow(dead_code)]
        /// Helper method to get a direct reference to a child widget's `WidgetState` from its `WidgetPod`.
        fn get_child_state<Child: Widget>(&self, child: &'_ WidgetPod<Child>) -> &'_ WidgetState {
            let child_state_ref = self
                .widget_state_children
                .get_child(child.id())
                .expect("get_child_state: child not found");
            child_state_ref.item
        }
    }
);

// Methods for all mutable context types
impl_context_method!(
    MutateCtx<'_>,
    EventCtx<'_>,
    UpdateCtx<'_>,
    LayoutCtx<'_>,
    ComposeCtx<'_>,
    {
        /// Helper method to get a mutable reference to a child widget's `WidgetState` from its `WidgetPod`.
        ///
        /// This one isn't defined for `PaintCtx` and `AccessCtx` because those contexts
        /// can't mutate `WidgetState`.
        fn get_child_state_mut<Child: Widget>(
            &mut self,
            child: &'_ mut WidgetPod<Child>,
        ) -> &'_ mut WidgetState {
            let child_state_mut = self
                .widget_state_children
                .get_child_mut(child.id())
                .expect("get_child_state_mut: child not found");
            child_state_mut.item
        }
    }
);

// --- MARK: GET LAYOUT ---
// Methods on all context types except LayoutCtx
// These methods access layout info calculated during the layout pass.
impl_context_method!(
    MutateCtx<'_>,
    QueryCtx<'_>,
    EventCtx<'_>,
    UpdateCtx<'_>,
    ComposeCtx<'_>,
    PaintCtx<'_>,
    AccessCtx<'_>,
    {
        /// The layout size.
        ///
        /// This is the layout size as ultimately determined by the parent
        /// container, on the previous layout pass.
        ///
        /// Generally it will be the same as the size returned by the child widget's
        /// [`layout`] method.
        ///
        /// [`layout`]: Widget::layout
        pub fn size(&self) -> Size {
            self.widget_state.size
        }

        pub fn layout_rect(&self) -> Rect {
            self.widget_state.layout_rect()
        }

        /// The origin of the widget in window coordinates, relative to the top left corner of the
        /// content area.
        pub fn window_origin(&self) -> Point {
            self.widget_state.window_origin()
        }

        pub fn window_layout_rect(&self) -> Rect {
            self.widget_state.window_layout_rect()
        }

        pub fn paint_rect(&self) -> Rect {
            self.widget_state.paint_rect()
        }

        /// The clip path of the widget, if any was set.
        ///
        /// For more information, see
        /// [`LayoutCtx::set_clip_path`](crate::LayoutCtx::set_clip_path).
        pub fn clip_path(&self) -> Option<Rect> {
            self.widget_state.clip_path
        }

        /// Convert a point from the widget's coordinate space to the window's.
        ///
        /// The returned point is relative to the content area; it excludes window chrome.
        pub fn to_window(&self, widget_point: Point) -> Point {
            self.window_origin() + widget_point.to_vec2()
        }
    }
);

// --- MARK: GET STATUS ---
// Methods on all context types except LayoutCtx
// Access status information (hovered/pointer captured/disabled/etc).
impl_context_method!(
    MutateCtx<'_>,
    QueryCtx<'_>,
    EventCtx<'_>,
    UpdateCtx<'_>,
    ComposeCtx<'_>,
    PaintCtx<'_>,
    AccessCtx<'_>,
    {
        /// The "hovered" status of a widget.
        ///
        /// A widget is "hovered" when the mouse is hovered over it. Widgets will
        /// often change their appearance as a visual indication that they
        /// will respond to mouse interaction.
        ///
        /// The hovered status is computed from the widget's layout rect. In a
        /// container hierarchy, all widgets with layout rects containing the
        /// mouse position have hovered status.
        ///
        /// Discussion: there is currently some confusion about whether a
        /// widget can be considered hovered when some other widget has captured the
        /// pointer (for example, when clicking one widget and dragging to the
        /// next). The documentation should clearly state the resolution.
        pub fn is_hovered(&self) -> bool {
            self.widget_state.is_hovered
        }

        /// Whether the pointer is captured by this widget.
        ///
        /// See [`capture_pointer`] for more information about pointer capture.
        ///
        /// [`capture_pointer`]: EventCtx::capture_pointer
        pub fn has_pointer_capture(&self) -> bool {
            self.global_state.pointer_capture_target == Some(self.widget_state.id)
        }

        /// The focus status of a widget.
        ///
        /// Returns `true` if this specific widget is focused.
        /// To check if any descendants are focused use [`has_focus`].
        ///
        /// Focus means that the widget receives keyboard events.
        ///
        /// A widget can request focus using the [`request_focus`] method.
        /// It's also possible to register for automatic focus via [`register_for_focus`].
        ///
        /// If a widget gains or loses focus it will get a [`Update::FocusChanged`] event.
        ///
        /// Only one widget at a time is focused. However due to the way events are routed,
        /// all ancestors of that widget will also receive keyboard events.
        ///
        /// [`request_focus`]: EventCtx::request_focus
        /// [`register_for_focus`]: UpdateCtx::register_for_focus
        /// [`Update::FocusChanged`]: crate::Update::FocusChanged
        /// [`has_focus`]: Self::has_focus
        pub fn is_focused(&self) -> bool {
            self.global_state.focused_widget == Some(self.widget_id())
        }

        /// The (tree) focus status of a widget.
        ///
        /// Returns `true` if either this specific widget or any one of its descendants is focused.
        /// To check if only this specific widget is focused use [`is_focused`](Self::is_focused).
        pub fn has_focus(&self) -> bool {
            self.widget_state.has_focus
        }

        /// Whether this widget gets pointer events and hovered status.
        pub fn accepts_pointer_interaction(&self) -> bool {
            self.widget_state.accepts_pointer_interaction
        }

        /// Whether this widget gets text focus.
        pub fn accepts_focus(&self) -> bool {
            self.widget_state.accepts_focus
        }

        /// Whether this widget gets IME events.
        pub fn accepts_text_input(&self) -> bool {
            self.widget_state.accepts_text_input
        }

        /// The disabled state of a widget.
        ///
        /// Returns `true` if this widget or any of its ancestors is explicitly disabled.
        /// To make this widget explicitly disabled use [`set_disabled`].
        ///
        /// Disabled means that this widget should not change the state of the application. What
        /// that means is not entirely clear but in any it should not change its data. Therefore
        /// others can use this as a safety mechanism to prevent the application from entering an
        /// illegal state.
        /// For an example the decrease button of a counter of type `usize` should be disabled if the
        /// value is `0`.
        ///
        /// [`set_disabled`]: EventCtx::set_disabled
        pub fn is_disabled(&self) -> bool {
            self.widget_state.is_disabled
        }

        /// Check is widget is stashed.
        ///
        /// **Note:** Stashed widgets are a WIP feature.
        pub fn is_stashed(&self) -> bool {
            self.widget_state.is_stashed
        }
    }
);

// --- MARK: CURSOR ---
// Cursor-related impls.
impl_context_method!(MutateCtx<'_>, EventCtx<'_>, UpdateCtx<'_>, {
    /// Notifies Masonry that the cursor returned by [`Widget::get_cursor`] has changed.
    ///
    /// This is mostly meant for cases where the cursor changes even if the pointer doesn't
    /// move, because the nature of the widget has changed somehow.
    pub fn cursor_icon_changed(&mut self) {
        trace!("cursor_icon_changed");
        self.global_state.needs_pointer_pass = true;
    }
});

// --- MARK: WIDGET_MUT ---
// Methods to get a child WidgetMut from a parent.
impl<'a> MutateCtx<'a> {
    /// Return a [`WidgetMut`] to a child widget.
    pub fn get_mut<'c, Child: Widget>(
        &'c mut self,
        child: &'c mut WidgetPod<Child>,
    ) -> WidgetMut<'c, Child> {
        let child_state_mut = self
            .widget_state_children
            .get_child_mut(child.id())
            .expect("get_mut: child not found");
        let child_mut = self
            .widget_children
            .get_child_mut(child.id())
            .expect("get_mut: child not found");
        let child_ctx = MutateCtx {
            global_state: self.global_state,
            parent_widget_state: Some(&mut self.widget_state),
            widget_state: child_state_mut.item,
            widget_state_children: child_state_mut.children,
            widget_children: child_mut.children,
        };
        WidgetMut {
            ctx: child_ctx,
            widget: child_mut.item.as_mut_dyn_any().downcast_mut().unwrap(),
        }
    }

    pub(crate) fn reborrow_mut(&mut self) -> MutateCtx<'_> {
        MutateCtx {
            global_state: self.global_state,
            // We don't don't reborrow `parent_widget_state`. This avoids running
            // `merge_up` in `WidgetMut::Drop` multiple times for the same state.
            // It will still be called when the original borrow is dropped.
            parent_widget_state: None,
            widget_state: self.widget_state,
            widget_state_children: self.widget_state_children.reborrow_mut(),
            widget_children: self.widget_children.reborrow_mut(),
        }
    }
}

// --- MARK: WIDGET_REF ---
// Methods to get a child WidgetRef from a parent.
impl<'w> QueryCtx<'w> {
    /// Return a [`WidgetRef`] to a child widget.
    pub fn get(self, child: WidgetId) -> WidgetRef<'w, dyn Widget> {
        let child_state = self
            .widget_state_children
            .into_child(child)
            .expect("get: child not found");
        let child = self
            .widget_children
            .into_child(child)
            .expect("get: child not found");

        let ctx = QueryCtx {
            global_state: self.global_state,
            widget_state_children: child_state.children,
            widget_children: child.children,
            widget_state: child_state.item,
        };

        WidgetRef {
            ctx,
            widget: child.item,
        }
    }
}

// --- MARK: UPDATE FLAGS ---
// Methods on MutateCtx, EventCtx, and UpdateCtx
impl_context_method!(MutateCtx<'_>, EventCtx<'_>, UpdateCtx<'_>, {
    /// Request a [`paint`](crate::Widget::paint) and an [`accessibility`](crate::Widget::accessibility) pass.
    pub fn request_render(&mut self) {
        trace!("request_render");
        self.widget_state.request_paint = true;
        self.widget_state.needs_paint = true;
        self.widget_state.needs_accessibility = true;
        self.widget_state.request_accessibility = true;
    }

    /// Request a [`paint`](crate::Widget::paint) pass.
    ///
    /// Unlike [`request_render`](Self::request_render), this does not request an [`accessibility`](crate::Widget::accessibility) pass.
    /// Use request_render unless you're sure an accessibility pass is not needed.
    pub fn request_paint_only(&mut self) {
        trace!("request_paint");
        self.widget_state.request_paint = true;
        self.widget_state.needs_paint = true;
    }

    /// Request an [`accessibility`](crate::Widget::accessibility) pass.
    ///
    /// This doesn't request a [`paint`](crate::Widget::paint) pass.
    /// If you want to request both an accessibility pass and a paint pass, use [`request_render`](Self::request_render).
    pub fn request_accessibility_update(&mut self) {
        trace!("request_accessibility_update");
        self.widget_state.needs_accessibility = true;
        self.widget_state.request_accessibility = true;
    }

    /// Request a layout pass.
    ///
    /// A Widget's [`layout`] method is always called when the widget tree
    /// changes, or the window is resized.
    ///
    /// If your widget would like to have layout called at any other time,
    /// (such as if it would like to change the layout of children in
    /// response to some event) it must call this method.
    ///
    /// [`layout`]: crate::Widget::layout
    pub fn request_layout(&mut self) {
        trace!("request_layout");
        self.widget_state.request_layout = true;
        self.widget_state.needs_layout = true;
    }

    // TODO - Document better
    /// Request a [`compose`] pass.
    ///
    /// The compose pass is often cheaper than the layout pass, because it can only transform individual widgets' position.
    /// [`compose`]: crate::Widget::compose
    pub fn request_compose(&mut self) {
        trace!("request_compose");
        self.widget_state.needs_compose = true;
        self.widget_state.request_compose = true;
    }

    /// Request an animation frame.
    pub fn request_anim_frame(&mut self) {
        trace!("request_anim_frame");
        self.widget_state.request_anim = true;
        self.widget_state.needs_anim = true;
    }

    /// Indicate that your children have changed.
    ///
    /// Widgets must call this method after adding a new child.
    pub fn children_changed(&mut self) {
        trace!("children_changed");
        self.widget_state.children_changed = true;
        self.widget_state.update_focus_chain = true;
        self.request_layout();
    }

    /// Indicate that a child is about to be removed from the tree.
    ///
    /// Container widgets should avoid dropping `WidgetPod`s. Instead, they should
    /// pass them to this method.
    pub fn remove_child(&mut self, child: WidgetPod<impl Widget>) {
        // TODO - Send recursive event to child
        let id = child.id();
        let _ = self
            .widget_state_children
            .remove_child(id)
            .expect("remove_child: child not found");
        let _ = self
            .widget_children
            .remove_child(id)
            .expect("remove_child: child not found");
        self.global_state.scenes.remove(&child.id());

        self.children_changed();
    }

    /// Set the disabled state for this widget.
    ///
    /// Setting this to `false` does not mean a widget is not still disabled; for instance it may
    /// still be disabled by an ancestor. See [`is_disabled`] for more information.
    ///
    /// [`is_disabled`]: EventCtx::is_disabled
    pub fn set_disabled(&mut self, disabled: bool) {
        self.widget_state.needs_update_disabled = true;
        self.widget_state.is_explicitly_disabled = disabled;
    }
});

// --- MARK: OTHER METHODS ---
// Methods on all context types except PaintCtx and AccessCtx
impl_context_method!(
    MutateCtx<'_>,
    EventCtx<'_>,
    UpdateCtx<'_>,
    LayoutCtx<'_>,
    ComposeCtx<'_>,
    {
        // TODO - Remove from MutateCtx?
        /// Queue a callback that will be called with a [`WidgetMut`] for this widget.
        ///
        /// The callbacks will be run in the order they were submitted during the mutate pass.
        pub fn mutate_self_later(
            &mut self,
            f: impl FnOnce(WidgetMut<'_, Box<dyn Widget>>) + Send + 'static,
        ) {
            let callback = MutateCallback {
                id: self.widget_state.id,
                callback: Box::new(f),
            };
            self.global_state.mutate_callbacks.push(callback);
        }

        /// Queue a callback that will be called with a [`WidgetMut`] for the given child widget.
        ///
        /// The callbacks will be run in the order they were submitted during the mutate pass.
        pub fn mutate_later<W: Widget>(
            &mut self,
            child: &mut WidgetPod<W>,
            f: impl FnOnce(WidgetMut<'_, W>) + Send + 'static,
        ) {
            let callback = MutateCallback {
                id: child.id(),
                callback: Box::new(|mut widget_mut| f(widget_mut.downcast())),
            };
            self.global_state.mutate_callbacks.push(callback);
        }

        /// Submit an [`Action`].
        ///
        /// Note: Actions are still a WIP feature.
        pub fn submit_action(&mut self, action: Action) {
            trace!("submit_action");
            self.global_state
                .emit_signal(RenderRootSignal::Action(action, self.widget_state.id));
        }

        /// Request a timer event.
        ///
        /// The return value is a token, which can be used to associate the
        /// request with the event.
        pub fn request_timer(&mut self, _deadline: Duration) -> TimerToken {
            todo!("request_timer");
        }

        /// Mark child widget as stashed.
        ///
        /// If `stashed` is true, the child will not be painted or listed in the accessibility tree.
        ///
        /// This will *not* trigger a layout pass.
        ///
        /// **Note:** Stashed widgets are a WIP feature.
        pub fn set_stashed(&mut self, child: &mut WidgetPod<impl Widget>, stashed: bool) {
            let child_state = self.get_child_state_mut(child);
            // Stashing is generally a property derived from the parent widget's state
            // (rather than set imperatively), so it is likely to be set as part of passes.
            // Therefore, we avoid re-running the update_stashed_pass in most cases.
            if child_state.is_explicitly_stashed != stashed {
                child_state.needs_update_stashed = true;
                child_state.is_explicitly_stashed = stashed;
            }
        }
    }
);

// FIXME - Remove
pub struct TimerToken;

impl EventCtx<'_> {
    // TODO - clearly document all semantics of pointer capture when they've been decided on
    // TODO - Figure out cases where widget should be notified of pointer capture
    // loss
    /// Capture the pointer in the current widget.
    ///
    /// Pointer capture is only allowed during a [`PointerDown`] event. It is a logic error to
    /// capture the pointer during any other event.
    ///
    /// A widget normally only receives pointer events when the pointer is inside the widget's
    /// layout box. Pointer capture causes widget layout boxes to be ignored: when the pointer is
    /// captured by a widget, that widget will continue receiving pointer events when the pointer
    /// is outside the widget's layout box. Other widgets the pointer is over will not receive
    /// events. Events that are not marked as handled by the capturing widget, bubble up to the
    /// widget's ancestors, ignoring their layout boxes as well.
    ///
    /// The pointer cannot be captured by multiple widgets at the same time. If a widget has
    /// captured the pointer and another widget captures it, the first widget loses the pointer
    /// capture.
    ///
    /// # Releasing the pointer
    ///
    /// Any widget can [`release`] the pointer during any event. The pointer is automatically
    /// released after handling of a [`PointerUp`] or [`PointerLeave`] event completes. A widget
    /// holding the pointer capture will be the target of these events.
    ///
    /// [`PointerDown`]: crate::PointerEvent::PointerDown
    /// [`PointerUp`]: crate::PointerEvent::PointerUp
    /// [`PointerLeave`]: crate::PointerEvent::PointerLeave
    /// [`release`]: Self::release_pointer
    #[track_caller]
    pub fn capture_pointer(&mut self) {
        debug_assert!(
            self.allow_pointer_capture,
            "Error in {}: event does not allow pointer capture",
            self.widget_id(),
        );
        // TODO: plumb pointer capture through to platform (through winit)
        self.global_state.pointer_capture_target = Some(self.widget_state.id);
    }

    /// Release the pointer previously captured through [`capture_pointer`].
    ///
    /// [`capture_pointer`]: EventCtx::capture_pointer
    pub fn release_pointer(&mut self) {
        self.global_state.pointer_capture_target = None;
    }

    /// Send a signal to parent widgets to scroll this widget into view.
    pub fn request_scroll_to_this(&mut self) {
        let rect = self.widget_state.layout_rect();
        self.global_state
            .scroll_request_targets
            .push((self.widget_state.id, rect));
    }

    /// Send a signal to parent widgets to scroll this area into view.
    ///
    /// `rect` is in local coordinates.
    pub fn request_scroll_to(&mut self, rect: Rect) {
        self.global_state
            .scroll_request_targets
            .push((self.widget_state.id, rect));
    }

    /// Set the event as "handled", which stops its propagation to other
    /// widgets.
    pub fn set_handled(&mut self) {
        trace!("set_handled");
        self.is_handled = true;
    }

    /// Determine whether the event has been handled by some other widget.
    pub fn is_handled(&self) -> bool {
        self.is_handled
    }

    /// The widget originally targeted by the event.
    ///
    /// This will be different from [`widget_id`](Self::widget_id) during event bubbling.
    pub fn target(&self) -> WidgetId {
        self.target
    }

    /// Request keyboard focus.
    ///
    /// Because only one widget can be focused at a time, multiple focus requests
    /// from different widgets during a single event cycle means that the last
    /// widget that requests focus will override the previous requests.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn request_focus(&mut self) {
        trace!("request_focus");
        // We need to send the request even if we're currently focused,
        // because we may have a sibling widget that already requested focus
        // and we have no way of knowing that yet. We need to override that
        // to deliver on the "last focus request wins" promise.
        let id = self.widget_id();
        self.global_state.next_focused_widget = Some(id);
    }

    /// Transfer focus to the widget with the given `WidgetId`.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn set_focus(&mut self, target: WidgetId) {
        trace!("set_focus target={:?}", target);
        self.global_state.next_focused_widget = Some(target);
    }

    /// Give up focus.
    ///
    /// This should only be called by a widget that currently has focus.
    ///
    /// See [`is_focused`](Self::is_focused) for more information about focus.
    pub fn resign_focus(&mut self) {
        trace!("resign_focus");
        if self.has_focus() {
            self.global_state.next_focused_widget = None;
        } else {
            warn!(
                "resign_focus can only be called by the currently focused widget {} \
                 or one of its ancestors.",
                self.widget_id()
            );
        }
    }
}

impl RegisterCtx<'_> {
    /// Register a child widget.
    ///
    /// Container widgets should call this on all their children in
    /// their implementation of [`Widget::register_children`].
    pub fn register_child(&mut self, child: &mut WidgetPod<impl Widget>) {
        let Some(widget) = child.take_inner() else {
            return;
        };

        #[cfg(debug_assertions)]
        {
            self.registered_ids.push(child.id());
        }

        let id = child.id();
        let state = WidgetState::new(child.id(), widget.short_type_name());

        self.widget_children.insert_child(id, Box::new(widget));
        self.widget_state_children.insert_child(id, state);
    }
}

// --- MARK: UPDATE LAYOUT ---
impl LayoutCtx<'_> {
    #[track_caller]
    fn assert_layout_done(&self, child: &WidgetPod<impl Widget>, method_name: &str) {
        if self.get_child_state(child).needs_layout {
            debug_panic!(
                "Error in {}: trying to call '{}' with child '{}' {} before computing its layout",
                self.widget_id(),
                method_name,
                self.get_child(child).short_type_name(),
                child.id(),
            );
        }
    }

    #[track_caller]
    fn assert_placed(&self, child: &WidgetPod<impl Widget>, method_name: &str) {
        if self.get_child_state(child).is_expecting_place_child_call {
            debug_panic!(
                "Error in {}: trying to call '{}' with child '{}' {} before placing it",
                self.widget_id(),
                method_name,
                self.get_child(child).short_type_name(),
                child.id(),
            );
        }
    }

    // TODO - Reorder methods so that methods necessary for layout
    // appear higher in documentation.

    /// Compute layout of a child widget.
    ///
    /// Container widgets must call this on every child as part of
    /// their [`layout`] method.
    ///
    /// [`layout`]: Widget::layout
    pub fn run_layout<W: Widget>(&mut self, child: &mut WidgetPod<W>, bc: &BoxConstraints) -> Size {
        run_layout_on(self, child, bc)
    }

    /// Set explicit paint [`Insets`] for this widget.
    ///
    /// You are not required to set explicit paint bounds unless you need
    /// to paint outside of your layout bounds. In this case, the argument
    /// should be an [`Insets`] struct that indicates where your widget
    /// needs to overpaint, relative to its bounds.
    ///
    /// For more information, see [`WidgetPod::paint_insets`].
    ///
    /// [`WidgetPod::paint_insets`]: crate::widget::WidgetPod::paint_insets
    pub fn set_paint_insets(&mut self, insets: impl Into<Insets>) {
        let insets = insets.into();
        self.widget_state.paint_insets = insets.nonnegative();
    }

    // TODO - This is currently redundant with the code in LayoutCtx::place_child
    /// Given a child and its parent's size, determine the
    /// appropriate paint `Insets` for the parent.
    ///
    /// This is a convenience method; it allows the parent to correctly
    /// propagate a child's desired paint rect, if it extends beyond the bounds
    /// of the parent's layout rect.
    ///
    /// ## Panics
    ///
    /// This method will panic if the child's [`layout()`](WidgetPod::layout) method has not been called yet
    /// and if [`LayoutCtx::place_child()`] has not been called for the child.
    #[track_caller]
    pub fn compute_insets_from_child(
        &mut self,
        child: &WidgetPod<impl Widget>,
        my_size: Size,
    ) -> Insets {
        self.assert_layout_done(child, "compute_insets_from_child");
        self.assert_placed(child, "compute_insets_from_child");
        let parent_bounds = Rect::ZERO.with_size(my_size);
        let union_paint_rect = self
            .get_child_state(child)
            .paint_rect()
            .union(parent_bounds);
        union_paint_rect - parent_bounds
    }

    /// Set an explicit baseline position for this widget.
    ///
    /// The baseline position is used to align widgets that contain text,
    /// such as buttons, labels, and other controls. It may also be used
    /// by other widgets that are opinionated about how they are aligned
    /// relative to neighbouring text, such as switches or checkboxes.
    ///
    /// The provided value should be the distance from the *bottom* of the
    /// widget to the baseline.
    pub fn set_baseline_offset(&mut self, baseline: f64) {
        self.widget_state.baseline_offset = baseline;
    }

    /// Returns whether this widget needs to call [`WidgetPod::layout`]
    pub fn needs_layout(&self) -> bool {
        self.widget_state.needs_layout
    }

    /// Returns whether a child of this widget needs to call [`WidgetPod::layout`]
    pub fn child_needs_layout(&self, child: &WidgetPod<impl Widget>) -> bool {
        self.get_child_state(child).needs_layout
    }

    /// The distance from the bottom of the given widget to the baseline.
    ///
    /// ## Panics
    ///
    /// This method will panic if [`WidgetPod::layout`] has not been called yet for
    /// the child.
    #[track_caller]
    pub fn child_baseline_offset(&self, child: &WidgetPod<impl Widget>) -> f64 {
        self.assert_layout_done(child, "child_baseline_offset");
        self.get_child_state(child).baseline_offset
    }

    /// Get the given child's layout rect.
    ///
    /// ## Panics
    ///
    /// This method will panic if [`WidgetPod::layout`] and [`LayoutCtx::place_child`]
    /// have not been called yet for the child.
    #[track_caller]
    pub fn child_layout_rect(&self, child: &WidgetPod<impl Widget>) -> Rect {
        self.assert_layout_done(child, "child_layout_rect");
        self.assert_placed(child, "child_layout_rect");
        self.get_child_state(child).layout_rect()
    }

    /// Get the given child's paint rect.
    ///
    /// ## Panics
    ///
    /// This method will panic if [`WidgetPod::layout`] and [`LayoutCtx::place_child`]
    /// have not been called yet for the child.
    #[track_caller]
    pub fn child_paint_rect(&self, child: &WidgetPod<impl Widget>) -> Rect {
        self.assert_layout_done(child, "child_paint_rect");
        self.assert_placed(child, "child_paint_rect");
        self.get_child_state(child).paint_rect()
    }

    /// Get the given child's size.
    ///
    /// ## Panics
    ///
    /// This method will panic if [`WidgetPod::layout`] has not been called yet for
    /// the child.
    #[track_caller]
    pub fn child_size(&self, child: &WidgetPod<impl Widget>) -> Size {
        self.assert_layout_done(child, "child_size");
        self.get_child_state(child).layout_rect().size()
    }

    /// Skips running the layout pass and calling `place_child` on the child.
    ///
    /// This may be removed in the future. Currently it's useful for
    /// stashed children and children whose layout is cached.
    pub fn skip_layout(&mut self, child: &mut WidgetPod<impl Widget>) {
        self.get_child_state_mut(child).request_layout = false;
    }

    /// Gives the widget a clip path.
    ///
    /// A widget's clip path will have two effects:
    /// - It serves as a mask for painting operations of the widget's children (*not* the widget itself).
    /// - Pointer events must be inside that path to reach the widget's children.
    pub fn set_clip_path(&mut self, path: Rect) {
        // We intentionally always log this because clip paths are:
        // 1) Relatively rare in the tree
        // 2) An easy potential source of items not being visible when expected
        trace!("set_clip_path {path:?}");
        self.widget_state.clip_path = Some(path);
        // TODO - Updating the clip path may have
        // other knock-on effects we'd need to document.
        self.widget_state.request_accessibility = true;
        self.widget_state.needs_accessibility = true;
        self.widget_state.needs_paint = true;
    }

    /// Remove the widget's clip path.
    ///
    /// See [`LayoutCtx::set_clip_path`] for details.
    pub fn clear_clip_path(&mut self) {
        trace!("clear_clip_path");
        self.widget_state.clip_path = None;
        // TODO - Updating the clip path may have
        // other knock-on effects we'd need to document.
        self.widget_state.request_accessibility = true;
        self.widget_state.needs_accessibility = true;
        self.widget_state.needs_paint = true;
    }

    /// Set the position of a child widget, in the parent's coordinate space.
    /// This will affect the parent's display rect.
    ///
    /// Container widgets must call this method with each non-stashed child in their
    /// layout method, after calling `child.layout(...)`.
    ///
    /// ## Panics
    ///
    /// This method will panic if [`WidgetPod::layout`] has not been called yet for
    /// the child.
    #[track_caller]
    pub fn place_child<W: Widget>(&mut self, child: &mut WidgetPod<W>, origin: Point) {
        self.assert_layout_done(child, "place_child");
        if origin.x.is_nan()
            || origin.x.is_infinite()
            || origin.y.is_nan()
            || origin.y.is_infinite()
        {
            debug_panic!(
                "Error in {}: trying to call 'place_child' with child '{}' {} with invalid origin {:?}",
                self.widget_id(),
                self.get_child(child).short_type_name(),
                child.id(),
                origin,
            );
        }
        if origin != self.get_child_state_mut(child).origin {
            self.get_child_state_mut(child).origin = origin;
            self.get_child_state_mut(child).translation_changed = true;
        }
        self.get_child_state_mut(child)
            .is_expecting_place_child_call = false;

        self.widget_state.local_paint_rect = self
            .widget_state
            .local_paint_rect
            .union(self.get_child_state(child).paint_rect());
    }
}

impl ComposeCtx<'_> {
    pub fn needs_compose(&self) -> bool {
        self.widget_state.needs_compose
    }

    /// Set a translation for the child widget.
    ///
    /// The translation is applied on top of the position from [`LayoutCtx::place_child`].
    pub fn set_child_translation<W: Widget>(
        &mut self,
        child: &mut WidgetPod<W>,
        translation: Vec2,
    ) {
        if translation.x.is_nan()
            || translation.x.is_infinite()
            || translation.y.is_nan()
            || translation.y.is_infinite()
        {
            debug_panic!(
                "Error in {}: trying to call 'set_child_translation' with child '{}' {} with invalid translation {:?}",
                self.widget_id(),
                self.get_child(child).short_type_name(),
                child.id(),
                translation,
            );
        }
        let child = self.get_child_state_mut(child);
        if translation != child.translation {
            child.translation = translation;
            child.translation_changed = true;
        }
    }
}

// --- MARK: OTHER STUFF ---
impl_context_method!(LayoutCtx<'_>, PaintCtx<'_>, {
    /// Get the contexts needed to build and paint text sections.
    pub fn text_contexts(&mut self) -> (&mut FontContext, &mut LayoutContext<TextBrush>) {
        (
            &mut self.global_state.font_context,
            &mut self.global_state.text_layout_context,
        )
    }
});

// --- MARK: RAW WRAPPERS ---
macro_rules! impl_get_raw {
    ($SomeCtx:tt) => {
        impl<'s> $SomeCtx<'s> {
            /// Get a child context and a raw shared reference to a child widget.
            ///
            /// The child context can be used to call context methods on behalf of the
            /// child widget.
            pub fn get_raw_ref<'a, 'r, Child: Widget>(
                &'a mut self,
                child: &'a mut WidgetPod<Child>,
            ) -> RawWrapper<'r, $SomeCtx<'r>, Child>
            where
                'a: 'r,
                's: 'r,
            {
                let child_state_mut = self
                    .widget_state_children
                    .get_child_mut(child.id())
                    .expect("get_raw_ref: child not found");
                let child_mut = self
                    .widget_children
                    .get_child_mut(child.id())
                    .expect("get_raw_ref: child not found");
                #[allow(clippy::needless_update)]
                let child_ctx = $SomeCtx {
                    widget_state: child_state_mut.item,
                    widget_state_children: child_state_mut.children,
                    widget_children: child_mut.children,
                    global_state: self.global_state,
                    ..*self
                };
                RawWrapper {
                    ctx: child_ctx,
                    widget: child_mut.item.as_dyn_any().downcast_ref().unwrap(),
                }
            }

            /// Get a raw mutable reference to a child widget.
            ///
            /// See documentation for [`AllowRawMut`] for more details.
            pub fn get_raw_mut<'a, 'r, Child: Widget + AllowRawMut>(
                &'a mut self,
                child: &'a mut WidgetPod<Child>,
            ) -> RawWrapperMut<'r, $SomeCtx<'r>, Child>
            where
                'a: 'r,
                's: 'r,
            {
                let child_state_mut = self
                    .widget_state_children
                    .get_child_mut(child.id())
                    .expect("get_raw_mut: child not found");
                let child_mut = self
                    .widget_children
                    .get_child_mut(child.id())
                    .expect("get_raw_mut: child not found");
                #[allow(clippy::needless_update)]
                let child_ctx = $SomeCtx {
                    widget_state: child_state_mut.item,
                    widget_state_children: child_state_mut.children,
                    widget_children: child_mut.children,
                    global_state: self.global_state,
                    ..*self
                };
                RawWrapperMut {
                    parent_widget_state: &mut self.widget_state,
                    ctx: child_ctx,
                    widget: child_mut.item.as_mut_dyn_any().downcast_mut().unwrap(),
                }
            }
        }
    };
}

impl_get_raw!(EventCtx);
impl_get_raw!(UpdateCtx);
impl_get_raw!(LayoutCtx);

impl<'s> AccessCtx<'s> {
    pub fn get_raw_ref<'a, 'r, Child: Widget>(
        &'a mut self,
        child: &'a WidgetPod<Child>,
    ) -> RawWrapper<'r, AccessCtx<'r>, Child>
    where
        'a: 'r,
        's: 'r,
    {
        let child_state_mut = self
            .widget_state_children
            .get_child_mut(child.id())
            .expect("get_raw_ref: child not found");
        let child_mut = self
            .widget_children
            .get_child_mut(child.id())
            .expect("get_raw_ref: child not found");
        let child_ctx = AccessCtx {
            widget_state: child_state_mut.item,
            widget_state_children: child_state_mut.children,
            widget_children: child_mut.children,
            global_state: self.global_state,
            tree_update: self.tree_update,
            rebuild_all: self.rebuild_all,
            scale_factor: self.scale_factor,
        };
        RawWrapper {
            ctx: child_ctx,
            widget: child_mut.item.as_dyn_any().downcast_ref().unwrap(),
        }
    }
}

pub struct RawWrapper<'a, Ctx, W> {
    ctx: Ctx,
    widget: &'a W,
}

pub struct RawWrapperMut<'a, Ctx: IsContext, W> {
    parent_widget_state: &'a mut WidgetState,
    ctx: Ctx,
    widget: &'a mut W,
}

impl<Ctx, W> RawWrapper<'_, Ctx, W> {
    pub fn widget(&self) -> &W {
        self.widget
    }

    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }
}

impl<Ctx: IsContext, W> RawWrapperMut<'_, Ctx, W> {
    pub fn widget(&mut self) -> &mut W {
        self.widget
    }

    pub fn ctx(&mut self) -> &mut Ctx {
        &mut self.ctx
    }
}

impl<'a, Ctx: IsContext, W> Drop for RawWrapperMut<'a, Ctx, W> {
    fn drop(&mut self) {
        self.parent_widget_state
            .merge_up(self.ctx.get_widget_state());
    }
}

mod private {
    #[allow(unnameable_types)] // reason: see https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/
    pub trait Sealed {}
}

// TODO - Rethink RawWrapper API
// We're exporting a trait with a method that returns a private type.
// It's mostly fine because the trait is sealed anyway, but it's not great for documentation.

#[allow(private_interfaces)] // reason: see https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/
pub trait IsContext: private::Sealed {
    fn get_widget_state(&mut self) -> &mut WidgetState;
}

macro_rules! impl_context_trait {
    ($SomeCtx:tt) => {
        impl private::Sealed for $SomeCtx<'_> {}

        #[allow(private_interfaces)] // reason: see https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/
        impl IsContext for $SomeCtx<'_> {
            fn get_widget_state(&mut self) -> &mut WidgetState {
                self.widget_state
            }
        }
    };
}

impl_context_trait!(EventCtx);
impl_context_trait!(UpdateCtx);
impl_context_trait!(LayoutCtx);
