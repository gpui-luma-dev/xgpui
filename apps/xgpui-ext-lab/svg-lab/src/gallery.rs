//! Native multi-element workload. Measures host frames, not GPU presentation latency.
use super::*;
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
use xgpui_svg::{SchedulerLimits, SvgScheduler};

const ROW: f32 = 250.;
const COLS: usize = 3;
const VIEW_HEIGHT: f32 = 590.;
const ART: &[&str] = &[
    "tiger",
    "gallardo",
    "AJ_Digital_Camera",
    "USStates",
    "rg1024_green_grapes",
    "rg1024_metal_effect",
    "dragon",
    "ny1",
];

fn phase(t: f64) -> &'static str {
    match t {
        t if t < 4. => "baseline",
        t if t < 12. => "zoom",
        t if t < 20. => "scroll",
        t if t < 28. => "mixed",
        t if t < 32. => "settle",
        _ => "unmounted",
    }
}
fn visible(index: usize, offset: f32, eager: bool) -> bool {
    eager
        || ((index / COLS) as f32 * ROW >= offset - ROW
            && (index / COLS) as f32 * ROW < offset + VIEW_HEIGHT + ROW)
}

#[cfg(target_os = "macos")]
fn memory() -> Value {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // libproc fills this versioned structure on success. Values are bytes.
    let rc = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V4,
            usage.as_mut_ptr().cast(),
        )
    };
    if rc != 0 {
        return json!({"error": "proc_pid_rusage failed"});
    }
    let r = unsafe { usage.assume_init() };
    json!({"rss_bytes":r.ri_resident_size,"physical_footprint_bytes":r.ri_phys_footprint,"peak_physical_footprint_bytes":r.ri_lifetime_max_phys_footprint})
}
#[cfg(not(target_os = "macos"))]
fn memory() -> Value {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::get_current_pid().unwrap();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        sysinfo::ProcessRefreshKind::new().with_memory(),
    );
    json!({"rss_bytes":system.process(pid).map(|p|p.memory())})
}

struct Gallery {
    docs: Vec<(String, SvgDocument)>,
    count: usize,
    eager: bool,
    policy: RasterPolicy,
    scheduler: Option<SvgScheduler>,
    scroll: ScrollHandle,
    zoom: f32,
    start: Instant,
    last: Instant,
    sample_at: f64,
    frames: Vec<Value>,
    samples: Vec<Value>,
    statuses: Rc<RefCell<Vec<Option<SvgStatus>>>>,
    report: Option<PathBuf>,
    baseline: Value,
    finished: bool,
}
impl Gallery {
    fn new(docs: Vec<(String, SvgDocument)>, options: benchmark::Options) -> Self {
        let now = Instant::now();
        Self {
            docs,
            count: options.gallery_tiles,
            eager: options.gallery_eager,
            policy: RasterPolicy {
                max_output_bytes: options.budget_mib * MIB,
                ..Default::default()
            },
            scheduler: options.scheduler_mib.map(|mib| {
                SvgScheduler::new(SchedulerLimits {
                    max_jobs: options.scheduler_jobs,
                    max_bytes: mib * MIB,
                })
                .expect("validated scheduler limits")
            }),
            scroll: ScrollHandle::new(),
            zoom: 1.,
            start: now,
            last: now,
            sample_at: 0.,
            frames: vec![],
            samples: vec![],
            statuses: Rc::new(RefCell::new(vec![None; options.gallery_tiles])),
            report: options.gallery_report,
            baseline: memory(),
            finished: false,
        }
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let t = now.duration_since(self.start).as_secs_f64();
        let dt = now.duration_since(self.last).as_secs_f64() * 1000.;
        self.last = now;
        if let Some(path) = self.report.as_ref().filter(|_| !self.finished) {
            self.frames
                .push(json!({"t":t,"phase":phase(t),"interval_ms":dt}));
            self.zoom = match phase(t) {
                "zoom" | "mixed" => 2f32.powf(((t * 1.5).sin() as f32 + 1.) * 2.),
                "scroll" | "settle" => 8.,
                _ => 1.,
            };
            if matches!(phase(t), "scroll" | "mixed") {
                let max = ((self.count.div_ceil(COLS) as f32) * ROW - VIEW_HEIGHT).max(0.);
                self.scroll
                    .set_offset(point(px(0.), px(-max * ((t * 0.9).sin() as f32 + 1.) / 2.)));
            }
            if t >= self.sample_at {
                self.sample_at = t + 0.1;
                let statuses = self.statuses.borrow();
                let offset = -f32::from(self.scroll.offset().y);
                let active: Vec<_> = (0..self.count)
                    .filter(|&i| t < 32. && visible(i, offset, self.eager))
                    .collect();
                let bytes: u64 = active
                    .iter()
                    .filter_map(|&i| statuses[i].as_ref()?.displayed)
                    .map(|s| i32::from(s.width) as u64 * i32::from(s.height) as u64 * 4)
                    .sum();
                self.samples.push(json!({"t":t,"phase":phase(t),"memory":memory(),"scheduler":self.scheduler.as_ref().map(|s| {let x=s.stats();json!({"members":x.members,"active_jobs":x.active_jobs,"queued_jobs":x.queued_jobs,"managed_bytes":x.managed_bytes,"retained_bytes":x.retained_bytes,"in_flight_bytes":x.in_flight_bytes,"peak_managed_bytes":x.peak_managed_bytes,"peak_active_jobs":x.peak_active_jobs})}),"zoom":self.zoom,"scroll_y":offset,"mounted":active.len(),"displaying":active.iter().filter(|&&i|statuses[i].as_ref().is_some_and(|s|s.displayed.is_some())).count(),"ready":active.iter().filter(|&&i|statuses[i].as_ref().is_some_and(|s|s.phase==xgpui_svg::SvgPhase::Ready)).count(),"displayed_rgba_bytes":bytes,"busy":active.iter().filter(|&&i|statuses[i].as_ref().is_some_and(|s| matches!(s.phase,xgpui_svg::SvgPhase::Preview|xgpui_svg::SvgPhase::Detail))).count(),"failed":active.iter().filter(|&&i|statuses[i].as_ref().is_some_and(|s|s.error.is_some())).count()}));
            }
            if t >= 36. {
                let result = json!({"schema":2,"scheduler":self.scheduler.as_ref().map(|s|json!({"max_jobs":s.limits().max_jobs,"max_bytes":s.limits().max_bytes})),"tiles":self.count,"eager":self.eager,"budget_bytes_per_element":self.policy.max_output_bytes,"density":window.scale_factor(),"window_logical":[1140,720],"artwork":self.docs.iter().map(|(n,_)|n).collect::<Vec<_>>(),"baseline_after_parse":self.baseline,"frames":self.frames,"samples":self.samples,"scope":"Native host render intervals, sampled whole-process RSS and macOS physical footprint. Includes process-accounted graphics allocations, not WindowServer or system-wide GPU memory. Automated camera/scroll changes, not input-to-photon latency."});
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).expect("report directory");
                }
                std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap())
                    .expect("write gallery report");
                self.finished = true;
                cx.quit();
            }
        }
        if self.report.is_some() {
            window.request_animation_frame();
        }
    }
}
impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let t = self.start.elapsed().as_secs_f64();
        let unmounted = self.report.is_some() && t >= 32.;
        let offset = -f32::from(self.scroll.offset().y);
        let mut rows = vec![];
        for row in 0..self.count.div_ceil(COLS) {
            let mut tiles = vec![];
            for index in row * COLS..((row + 1) * COLS).min(self.count) {
                let (name, doc) = &self.docs[index % self.docs.len()];
                let mut tile = div()
                    .w(px(360.))
                    .h(px(ROW))
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!("{} / {}", index + 1, name));
                if !unmounted && visible(index, offset, self.eager) {
                    let statuses = self.statuses.clone();
                    let mut image = svg_image(("gallery-svg", index), doc.clone())
                        .h(px(210.))
                        .zoom(self.zoom)
                        .policy(self.policy)
                        .bg(rgb(0xffffff))
                        .on_status_change(move |s, _, _| {
                            statuses.borrow_mut()[index] = Some(s.clone())
                        });
                    if let Some(scheduler) = &self.scheduler {
                        image = image.scheduler(scheduler.clone());
                    }
                    tile = tile.child(image);
                }
                tiles.push(tile);
            }
            rows.push(div().flex().h(px(ROW)).flex_shrink_0().children(tiles));
        }
        div().size_full().bg(rgb(0x101827)).text_color(rgb(0xe2e8f0)).p_4().flex().flex_col().gap_2()
            .child(format!("SVG gallery • {} tiles • {} • {} MiB / element • {:.1}× • {}",self.count,if self.eager{"eager"}else{"visible + overscan"},self.policy.max_output_bytes/MIB,self.zoom,if self.report.is_some(){phase(t)}else{"interactive"}))
            .child("Scroll to explore • hold Command and wheel to zoom • automated reports exercise all elements concurrently")
            .child(div().id("gallery-scroll").h(px(VIEW_HEIGHT)).overflow_y_scroll().track_scroll(&self.scroll)
                .on_scroll_wheel(cx.listener(|this,event:&ScrollWheelEvent,_,cx| {
                    if event.modifiers.platform {let delta=event.delta.pixel_delta(px(16.));this.zoom=(this.zoom*(-f32::from(delta.y)*0.005).exp()).clamp(0.25,16.);cx.stop_propagation();}
                    cx.notify();

                })).children(rows))
    }
}
pub fn run(options: benchmark::Options) -> Result<()> {
    ensure!(
        (1..=96).contains(&options.gallery_tiles),
        "gallery tiles must be 1..=96"
    );
    ensure!(
        (1..=32).contains(&options.budget_mib),
        "gallery output budget must be 1..=32 MiB"
    );
    if let Some(mib) = options.scheduler_mib {
        ensure!((1..=1024).contains(&mib), "scheduler MiB must be 1..=1024");
        SvgScheduler::new(SchedulerLimits {
            max_jobs: options.scheduler_jobs,
            max_bytes: mib * MIB,
        })?;
    }
    let renderer = SvgRenderer::new(Arc::new(()));
    let directory = options.gallery.as_ref().unwrap();
    let docs = ART
        .iter()
        .filter(|name| directory.join(format!("{name}.svg")).is_file())
        .map(|name| {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(directory.join(format!("{name}.svg")))?
                .take((32 * MIB + 1) as u64)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 32 * MIB, "input too large");
            Ok((name.to_string(), SvgDocument::parse(&renderer, &bytes)?))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !docs.is_empty(),
        "No gallery corpus SVGs found in {}",
        directory.display()
    );
    gpui_platform::application().run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1140.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("XGPUI SVG Gallery Stress");
                cx.new(|_| Gallery::new(docs, options))
            },
        )
        .expect("gallery window");
        cx.activate(true);
    });
    Ok(())
}
