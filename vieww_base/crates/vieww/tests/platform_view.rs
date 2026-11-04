//! `PlatformView`: the widget-tree half of FW-20. A recording
//! `PlatformViews` stands in for the OS, so what is asserted is exactly the
//! calls a real backend would receive.

use std::cell::RefCell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww::{BuildContext, Widget, WidgetKind, WidgetNode};
use vieww_element::Signal;
use vieww_foundation::capability::{
    PlatformViewHandle, PlatformViewId, PlatformViewSpec, PlatformViews,
};
use vieww_foundation::{EdgeInsets, Rect, ServiceError, Size};
use vieww_render::FrameDriver;
use vieww_widget::{PlatformView, PlatformViewController, SizedBox};

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Create(String, Rect),
    Move(u64, Rect),
    Dispose(u64),
}

#[derive(Default)]
struct Recorder {
    calls: Rc<RefCell<Vec<Call>>>,
    next: RefCell<u64>,
}

impl PlatformViews for Recorder {
    fn registered(&self) -> Vec<String> {
        vec!["test".to_owned()]
    }

    fn create(
        &self,
        spec: &PlatformViewSpec,
        bounds: Rect,
    ) -> Result<PlatformViewHandle, ServiceError> {
        let id = {
            let mut next = self.next.borrow_mut();
            *next += 1;
            *next
        };
        self.calls
            .borrow_mut()
            .push(Call::Create(spec.kind.clone(), bounds));
        let calls = Rc::clone(&self.calls);
        Ok(PlatformViewHandle::new(PlatformViewId(id), move |id| {
            calls.borrow_mut().push(Call::Dispose(id.0));
        }))
    }

    fn set_bounds(&self, id: PlatformViewId, bounds: Rect) -> Result<(), ServiceError> {
        self.calls.borrow_mut().push(Call::Move(id.0, bounds));
        Ok(())
    }

    fn send(&self, _id: PlatformViewId, message: &str) -> Result<String, ServiceError> {
        Ok(format!("echo {message}"))
    }
}

#[derive(Debug, Clone)]
struct Screen {
    controller: PlatformViewController,
    inset: Signal<f32>,
    shown: Signal<bool>,
    rebuilds: Signal<u32>,
}

impl Widget for Screen {
    fn debug_name(&self) -> &'static str {
        "Screen"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let _ = self.rebuilds.get();
        if !self.shown.get() {
            return SizedBox::shrink().into();
        }
        let inset = self.inset.get();
        // Loose constraints, so the view is the SizedBox's size rather than
        // the window's (the root hands down tight ones).
        Align::new(vieww_foundation::Alignment::TOP_LEFT)
            .child(
                Padding::new(EdgeInsets::only(inset, inset / 2.0, 0.0, 0.0)).child(
                    SizedBox::from_size(Size::new(120.0, 80.0))
                        .child(PlatformView::new(self.controller.clone())),
                ),
            )
            .into()
    }
}
vieww_widget::widget_node_from!(Screen);

fn mounted() -> (FrameDriver, Screen, Rc<RefCell<Vec<Call>>>) {
    let recorder = Recorder::default();
    let calls = Rc::clone(&recorder.calls);
    let service: Rc<dyn PlatformViews> = Rc::new(recorder);
    let mut driver = FrameDriver::new(Size::new(400.0, 300.0));
    let runtime = driver.elements().runtime().clone();
    let screen = Screen {
        controller: PlatformViewController::new(service, PlatformViewSpec::new("test")),
        inset: runtime.signal(20.0),
        shown: runtime.signal(true),
        rebuilds: runtime.signal(0),
    };
    driver.set_root(screen.clone());
    driver.draw_frame();
    (driver, screen, calls)
}

#[test]
fn the_view_is_created_where_layout_put_it() {
    let (_driver, screen, calls) = mounted();
    assert_eq!(
        *calls.borrow(),
        [Call::Create(
            "test".into(),
            Rect::new(20.0, 10.0, 140.0, 90.0)
        )]
    );
    assert!(screen.controller.is_created());
    assert_eq!(screen.controller.send("hi"), Ok("echo hi".to_owned()));
}

#[test]
fn a_move_is_one_set_bounds_and_an_unchanged_rebuild_is_none() {
    let (mut driver, screen, calls) = mounted();
    calls.borrow_mut().clear();
    screen.rebuilds.set(1);
    driver.draw_frame();
    driver.draw_frame();
    assert!(
        calls.borrow().is_empty(),
        "nothing moved: {:?}",
        calls.borrow()
    );

    screen.inset.set(40.0);
    driver.draw_frame();
    assert_eq!(
        *calls.borrow(),
        [Call::Move(1, Rect::new(40.0, 20.0, 160.0, 100.0))],
        "one move, no re-create"
    );
}

#[test]
fn leaving_the_tree_disposes_the_view_and_returning_makes_a_new_one() {
    let (mut driver, screen, calls) = mounted();
    calls.borrow_mut().clear();
    screen.shown.set(false);
    driver.draw_frame();
    assert_eq!(*calls.borrow(), [Call::Dispose(1)]);
    assert!(!screen.controller.is_created());

    calls.borrow_mut().clear();
    screen.shown.set(true);
    driver.draw_frame();
    assert_eq!(
        *calls.borrow(),
        [Call::Create(
            "test".into(),
            Rect::new(20.0, 10.0, 140.0, 90.0)
        )]
    );
}

#[test]
fn a_refusing_platform_is_reported_not_fatal() {
    let mut driver = FrameDriver::new(Size::new(200.0, 200.0));
    let service: Rc<dyn PlatformViews> = Rc::new(vieww_foundation::capability::NoPlatformViews);
    let controller = PlatformViewController::new(service, PlatformViewSpec::new("x"));
    driver.set_root(
        SizedBox::from_size(Size::new(50.0, 50.0)).child(PlatformView::new(controller.clone())),
    );
    driver.draw_frame();
    assert!(!controller.is_created());
    assert!(controller.error().is_some_and(|e| e.is_permanent()));
}
