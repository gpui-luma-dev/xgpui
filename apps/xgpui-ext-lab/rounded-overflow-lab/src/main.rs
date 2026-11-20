//! #37: actual GPUI descendant backgrounds under legacy and rounded overflow.
use anyhow::{Result, ensure};
use clap::Parser;
use gpui::{prelude::*, *};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use xgpui_ext_lab::{LabApp, LabWindowTitle};
use xgpui_shader::{Gradient, GradientKind, GradientStop, Paint, RoundedOverflow, painted};

const FONT: &[u8] = include_bytes!(
    "../../../../gpui/crates/gpui-web/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
);
const BACKGROUND: u32 = 0x111923;
#[derive(Parser)]
struct Options {
    /// Read back the real window, check clipped pixels/input and retained redraw, then exit.
    #[arg(long)]
    check: bool,
    #[arg(long, default_value = "/tmp/rounded-overflow-lab.png")]
    png: std::path::PathBuf,
}
struct ClipElement {
    child: AnyElement,
    effect: Option<Rc<RefCell<RoundedOverflow>>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}
impl IntoElement for ClipElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ClipElement {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.bounds.set(bounds);
        if let Some(effect) = &self.effect {
            effect
                .borrow()
                .prepaint(window, bounds, |window| self.child.prepaint(window, cx))
                .expect("rounded prepaint");
        } else {
            self.child.prepaint(window, cx);
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(effect) = &self.effect {
            effect
                .borrow_mut()
                .paint(window, bounds, |window| self.child.paint(window, cx))
                .expect("rounded paint");
        } else {
            self.child.paint(window, cx);
        }
    }
}
struct Lab {
    radius: f32,
    gradient: bool,
    alternate: bool,
    effect: Rc<RefCell<RoundedOverflow>>,
    bounds: [Rc<Cell<Bounds<Pixels>>>; 2],
    clicks: [Rc<Cell<usize>>; 2],
    status: SharedString,
}
impl Lab {
    fn new() -> Self {
        Self {
            radius: 48.,
            gradient: true,
            alternate: false,
            effect: Rc::new(RefCell::new(
                RoundedOverflow::new(Corners::all(px(46.))).unwrap(),
            )),
            bounds: std::array::from_fn(|_| Rc::new(Cell::new(Bounds::default()))),
            clicks: std::array::from_fn(|_| Rc::new(Cell::new(0))),
            status: "Click the child header near each corner, then inside its visible area.".into(),
        }
    }
    fn control(
        &self,
        title: &'static str,
        key: &'static str,
        action: impl Fn(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(key)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x2a3c50))
            .cursor_pointer()
            .child(title)
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this);
                cx.notify();
            }))
    }
    fn sample(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = if self.alternate {
            [[0.2, 0.85, 0.55, 1.], [0.95, 0.65, 0.12, 1.]]
        } else {
            [[0.15, 0.55, 0.95, 1.], [0.7, 0.2, 0.85, 1.]]
        };
        let paint: Paint = if self.gradient {
            Arc::new(
                Gradient::new(
                    GradientKind::Linear { angle: 125. },
                    vec![
                        GradientStop::new(0., colors[0]),
                        GradientStop::new(1., colors[1]),
                    ],
                )
                .unwrap(),
            )
            .into()
        } else {
            rgb(if self.alternate { 0x32bc87 } else { 0x4c6db8 }).into()
        };
        let count = self.clicks[index].clone();
        let child = painted()
            .relative()
            .w_full()
            .h_full()
            .overflow_hidden()
            .bg_paint(paint)
            .child(
                div()
                    .id(("child-header", index))
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(48.))
                    .bg(rgb(0xf28d72))
                    .text_color(rgb(0x241c26))
                    .px_3()
                    .py_3()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0xffc28f)))
                    .child("Square child background · click me")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        count.set(count.get() + 1);
                        this.status = format!(
                            "{} child clicked {} time(s)",
                            if index == 0 { "Legacy" } else { "Rounded" },
                            count.get()
                        )
                        .into();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .left(px(-12.))
                    .top(px(73.))
                    .text_size(px(22.))
                    .whitespace_nowrap()
                    .child("Gradient + ordinary GPUI text →"),
            )
            .child(
                div()
                    .absolute()
                    .right(px(-16.))
                    .bottom(px(-12.))
                    .w(px(104.))
                    .h(px(66.))
                    .bg(rgb(0x77e2d2))
                    .text_color(rgb(0x183b40))
                    .p_3()
                    .child("Overflowing child"),
            );
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(if index == 0 {
                "Existing overflow_hidden()"
            } else {
                "Experimental rounded overflow"
            })
            .child(
                div()
                    .w(px(284.))
                    .h(px(194.))
                    .rounded(px(self.radius))
                    .border_2()
                    .border_color(rgb(0xe5eaf0))
                    .bg(rgb(0x243449))
                    .child(ClipElement {
                        child: child.into_any_element(),
                        effect: if index == 1 {
                            Some(self.effect.clone())
                        } else {
                            None
                        },
                        bounds: self.bounds[index].clone(),
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xa9b7c7))
                    .w(px(284.))
                    .child(if index == 0 {
                        "Rectangle clipping leaves child colors in the rounded corners."
                    } else {
                        "The container clips the actual gradient, text and child backgrounds."
                    }),
            )
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The outer border uses GPUI's existing clamp; uniform border inset is 2px.
        let inner = (self.radius.min(97.) - 2.).max(0.);
        self.effect
            .borrow_mut()
            .set_radii(Corners::all(px(inner)))
            .unwrap();
        div().size_full().bg(rgb(BACKGROUND)).text_color(rgb(0xe5eaf0)).p_6().flex().flex_col().gap_4()
            .child(div().text_2xl().child("Rounded control backgrounds · issue #37"))
            .child("The same gradient and square child backgrounds inside the same rounded border.")
            .child(div().flex().gap_2()
                .child(self.control("Radius 0","radius-0",|s|s.radius=0.,cx))
                .child(self.control("Radius 12","radius-12",|s|s.radius=12.,cx))
                .child(self.control("Radius 48","radius-48",|s|s.radius=48.,cx))
                .child(self.control("Radius 96","radius-96",|s|s.radius=96.,cx))
                .child(self.control("Solid / gradient","gradient",|s|s.gradient=!s.gradient,cx))
                .child(self.control("Change colors","colors",|s|s.alternate=!s.alternate,cx)))
            .child(div().text_sm().child(format!("Configured outer radius: {:.0}px · uniform border: 2px",self.radius)))
            .child(div().flex().gap_8().child(self.sample(0,cx)).child(self.sample(1,cx)))
            .child(div().p_3().rounded_md().bg(rgb(0x203144)).child(self.status.clone()))
            .child(div().text_sm().text_color(rgb(0xa9b7c7)).child("Experimental capture path: grayscale text, two textures per container, no nested captures. Existing overflow_hidden() stays unchanged."))
    }
}
fn click(window: &mut Window, cx: &mut App, p: Point<Pixels>) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: p,
            button: MouseButton::Left,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button: MouseButton::Left,
            ..Default::default()
        }),
        cx,
    );
}
fn main() {
    let options = Options::parse();
    let app = gpui_platform::application();
    app.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(FONT)])
        .unwrap();
    app.run(move |cx| {
        let state=Rc::new(RefCell::new(None));
        let result=state.clone();
        let handle=cx.open_lab_window(WindowOptions{window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(940.),px(670.)),cx))),window_min_size:Some(size(px(900.),px(630.))),..Default::default()},move |window,cx|{
            window.set_lab_title("Rounded overflow · issue 37");
            let entity=cx.new(|_|Lab::new());result.replace(Some(entity.clone()));entity
        }).expect("open rounded overflow lab");
        cx.activate(true);
        if options.check {
            cx.spawn(async move |cx| {
                let entity=state.borrow().as_ref().unwrap().clone();
                let run:Result<()> = async {
                    cx.background_executor().timer(Duration::from_millis(750)).await;
                    let (bounds,counts)=entity.read_with(cx,|s,_|(s.bounds.each_ref().map(|b|b.get()),s.clicks.clone()));
                    ensure!(bounds.iter().all(|b|b.size.width>px(200.)),"lab did not lay out");
                    handle.update(cx,|_,window,cx|{
                        for b in bounds {click(window,cx,b.origin+point(px(3.),px(3.)));}
                    })?;
                    ensure!(counts[0].get()==1 && counts[1].get()==0,"corner clicks: expected legacy=1 rounded=0, got {}/{}",counts[0].get(),counts[1].get());
                    handle.update(cx,|_,window,cx| click(window,cx,bounds[1].origin+point(px(80.),px(20.))))?;
                    ensure!(counts[1].get()==1,"visible rounded header must receive click");
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    let (image,scale)=handle.update(cx,|_,window,_|(window.render_to_image(),window.scale_factor()))?;
                    let image=image?;image.save(&options.png)?;
                    let pixel=|i:usize|{let p=bounds[i].origin+point(px(3.),px(3.));image.get_pixel((f32::from(p.x)*scale) as u32,(f32::from(p.y)*scale) as u32).0};
                    let left=pixel(0);let right=pixel(1);
                    ensure!(left[0]>180 && left[1]>75,"legacy child corner must remain visible: {left:?}");
                    ensure!(right[0]<70 && right[1]<80 && right[2]<95,"rounded corner must reveal background: {right:?}");
                    // A radius-only change must invalidate both input and output geometry.
                    entity.update(cx,|s,cx|{s.radius=0.;cx.notify();});
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    handle.update(cx,|_,window,cx|click(window,cx,bounds[1].origin+point(px(3.),px(3.))))?;
                    ensure!(counts[1].get()==2,"zero-radius corner must become interactive");
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    let zero=handle.update(cx,|_,window,_|window.render_to_image())??;
                    let p=bounds[1].origin+point(px(3.),px(3.));
                    let zero_corner=zero.get_pixel((f32::from(p.x)*scale) as u32,(f32::from(p.y)*scale) as u32).0;
                    ensure!(zero_corner[0]>180 && zero_corner[1]>75,"radius-only paint update failed: {zero_corner:?}");
                    entity.update(cx,|s,cx|{s.radius=48.;s.gradient=false;s.alternate=true;cx.notify();});
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    let solid=handle.update(cx,|_,window,_|window.render_to_image())??;
                    let p=bounds[1].origin+point(px(80.),px(60.));
                    let color=solid.get_pixel((f32::from(p.x)*scale) as u32,(f32::from(p.y)*scale) as u32).0;
                    ensure!(color[1]>150 && color[0]<100,"configured solid background did not update: {color:?}");
                    entity.update(cx,|s,cx|{s.gradient=true;s.alternate=false;cx.notify();});
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    handle.update(cx,|_,window,_|window.render_to_image())??.save(&options.png)?;

                    println!("rounded-overflow: PASS real subtree pixels, corner/visible clicks, radius-only paint/input invalidation and configured background changes; capture {}",options.png.display());
                    Ok(())
                }.await;
                if let Err(error)=run {eprintln!("rounded-overflow: FAIL {error:#}");std::process::exit(1);}
                cx.update(|cx| cx.quit());
            }).detach();
        }
    });
}
