use crate::{
    app::App,
    event::{Event, EventHandler},
    msg::Msg,
    tui::Tui,
};

pub fn run(tui: &mut Tui, app: &mut App, api_base: &str) -> anyhow::Result<()> {
    // The first frame is drawn by `main` before the loop. Each turn then
    // updates from one event and draws that state, matching update-then-view.
    while !app.should_quit {
        match tui.events.next()? {
            Event::Tick => apply(app, Msg::Tick, api_base, &tui.events),
            Event::Key(key) => {
                if let Some(msg) = crate::keymap::route_key(app, key) {
                    apply(app, msg, api_base, &tui.events);
                }
            }
            Event::Paste(text) => apply(app, Msg::Paste(text), api_base, &tui.events),
            Event::Msg(msg) => apply(app, msg, api_base, &tui.events),
            Event::Mouse | Event::Resize => {}
        }
        if app.should_quit {
            break;
        }
        tui.draw(app)?;
    }
    Ok(())
}

fn apply(app: &mut App, msg: Msg, api_base: &str, events: &EventHandler) {
    for effect in crate::app::update::update(app, msg) {
        crate::effects::run(effect, api_base, events.sender());
    }
}
