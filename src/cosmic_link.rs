//! A lost signal travels between two planets, with impact at 68% of the route.
use gtk4::{cairo, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    f64::consts::TAU,
    rc::Rc,
};

const IMPACT: f64 = 2.8;
const CYCLE: f64 = 5.6;

fn progress(seconds: f64) -> (f64, f64) {
    let time = seconds.rem_euclid(CYCLE);
    (0.68 * (time / IMPACT).min(1.0), (time - IMPACT).max(0.0))
}

pub fn animation() -> gtk4::DrawingArea {
    let area = gtk4::DrawingArea::new();
    area.set_content_width(180);
    area.set_content_height(150);
    area.set_hexpand(true);
    area.update_property(&[gtk4::accessible::Property::Label(
        "Connection lost: a spaceship is struck by a meteor between two planets",
    )]);
    let time = Rc::new(Cell::new(0.0));
    area.set_draw_func({
        let time = Rc::clone(&time);
        move |_, cr, w, h| {
            draw(cr, w as f64, h as f64, time.get());
        }
    });
    let timer = Rc::new(RefCell::new(None::<gtk4::TickCallbackId>));
    area.connect_map({
        let timer = Rc::clone(&timer);
        let time = Rc::clone(&time);
        move |area| {
            let start = Cell::new(None::<i64>);
            let time = Rc::clone(&time);
            *timer.borrow_mut() = Some(area.add_tick_callback(move |area, clock| {
                if !area.settings().is_gtk_enable_animations() {
                    time.set(IMPACT + 0.3);
                    area.queue_draw();
                    return glib::ControlFlow::Break;
                }
                let now = clock.frame_time();
                let first = start.get().unwrap_or(now);
                start.set(Some(first));
                time.set((now - first) as f64 / 1_000_000.0);
                area.queue_draw();
                glib::ControlFlow::Continue
            }));
        }
    });
    area.connect_unmap(move |_| {
        if let Some(timer) = timer.borrow_mut().take() {
            timer.remove();
        }
    });
    area
}

fn circle(cr: &cairo::Context, x: f64, y: f64, radius: f64, color: (f64, f64, f64, f64)) {
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.fill();
}

fn draw(cr: &cairo::Context, width: f64, height: f64, seconds: f64) {
    let _ = cr.save();
    cr.scale(width / 480.0, height / 150.0);
    for i in 0..27 {
        circle(
            cr,
            ((i * 83 + 11) % 480) as f64,
            ((i * 37 + 9) % 145) as f64,
            if i % 4 == 0 { 1.4 } else { 0.7 },
            (0.73, 0.72, 0.91, 0.5),
        );
    }
    let (left, right, y) = (55.0, 425.0, 83.0);
    cr.set_source_rgba(0.63, 0.56, 0.85, 0.35);
    cr.set_line_width(1.0);
    cr.set_dash(&[3.0, 7.0], 0.0);
    cr.move_to(left + 28.0, y);
    cr.line_to(right - 28.0, y);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    for (x, color) in [(left, (0.45, 0.34, 0.83)), (right, (0.24, 0.61, 0.72))] {
        circle(cr, x, y, 34.0, (color.0, color.1, color.2, 0.10));
        circle(cr, x, y, 26.0, (color.0, color.1, color.2, 1.0));
        circle(cr, x - 7.0, y - 8.0, 11.0, (0.9, 0.9, 1.0, 0.13));
        let _ = cr.save();
        cr.translate(x, y);
        cr.rotate(-0.35);
        cr.scale(1.0, 0.32);
        cr.set_source_rgba(0.79, 0.73, 1.0, 0.65);
        cr.set_line_width(3.0);
        cr.arc(0.0, 0.0, 37.0, 0.0, TAU);
        let _ = cr.stroke();
        let _ = cr.restore();
    }
    let (travel, explosion) = progress(seconds);
    let start = left + 35.0;
    let route = right - 35.0 - start;
    let ship_x = start + travel * route;
    let time = seconds.rem_euclid(CYCLE);
    if time < IMPACT {
        let _ = cr.save();
        cr.translate(ship_x, y);
        cr.set_source_rgba(0.68, 0.45, 1.0, 0.4);
        cr.move_to(-12.0, -4.0);
        cr.line_to(-30.0 - (seconds * 23.0).sin() * 4.0, 0.0);
        cr.line_to(-12.0, 4.0);
        cr.close_path();
        let _ = cr.fill();
        cr.set_source_rgb(0.9, 0.91, 1.0);
        cr.move_to(15.0, 0.0);
        cr.curve_to(0.0, -13.0, -12.0, -9.0, -12.0, -5.0);
        cr.line_to(-18.0, -12.0);
        cr.line_to(-15.0, 0.0);
        cr.line_to(-18.0, 12.0);
        cr.line_to(-12.0, 5.0);
        cr.curve_to(-12.0, 9.0, 0.0, 13.0, 15.0, 0.0);
        let _ = cr.fill();
        circle(cr, 2.0, 0.0, 3.5, (0.3, 0.64, 0.93, 1.0));
        let _ = cr.restore();
        if time > 1.6 {
            let t = ((time - 1.6) / (IMPACT - 1.6)).clamp(0.0, 1.0);
            let x = start + 0.68 * route + 65.0 * (1.0 - t);
            let my = y - 105.0 * (1.0 - t);
            cr.set_source_rgba(1.0, 0.57, 0.32, 0.35);
            cr.move_to(x, my);
            cr.line_to(x + 21.0, my - 33.0);
            cr.set_line_width(7.0);
            let _ = cr.stroke();
            circle(cr, x, my, 7.0, (0.95, 0.58, 0.38, 1.0));
        }
    } else {
        let alpha = (1.0 - explosion / 2.3).max(0.0);
        circle(
            cr,
            ship_x,
            y,
            12.0 + explosion * 24.0,
            (0.95, 0.48, 0.36, alpha * 0.15),
        );
        for i in 0..10 {
            let angle = i as f64 * TAU / 10.0;
            let distance = 8.0 + explosion * (17.0 + (i % 3) as f64 * 8.0);
            circle(
                cr,
                ship_x + angle.cos() * distance,
                y + angle.sin() * distance,
                2.7 * alpha,
                (1.0, 0.64, 0.43, alpha),
            );
        }
    }
    let _ = cr.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collision_happens_between_halfway_and_eighty_percent() {
        assert_eq!(progress(0.0), (0.0, 0.0));
        assert!(progress(2.0).0 < progress(IMPACT).0);
        assert!((0.5..=0.8).contains(&progress(IMPACT).0));
        assert_eq!(progress(IMPACT + 1.0).0, progress(IMPACT).0);
        assert!(progress(IMPACT + 1.0).1 > 0.0);
        assert_eq!(progress(CYCLE), progress(0.0));
    }
}
