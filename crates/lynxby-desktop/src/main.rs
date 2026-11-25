mod app;
mod i18n;
mod screens;
mod theme;
mod widgets;

use gpui::*;
use lynxby_client::store::Store;
use lynxby_client::Config;

fn main() {
    let config = match Config::from_process() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    let store = Store::default_store();
    i18n::set_from_system();

    Application::new().run(move |cx: &mut App| {
        // The product font ships with the app (SIL OFL, see assets/fonts), so
        // weights render on machines that do not have Manrope installed.
        cx.text_system()
            .add_fonts(vec![
                include_bytes!("../assets/fonts/Manrope-Regular.ttf")
                    .as_slice()
                    .into(),
                include_bytes!("../assets/fonts/Manrope-SemiBold.ttf")
                    .as_slice()
                    .into(),
            ])
            .expect("load the bundled Manrope font");

        let bounds = Bounds::centered(None, size(px(1120.), px(780.)), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(880.), px(600.))),
            app_id: Some("com.lynxby.desktop".to_string()),
            titlebar: Some(TitlebarOptions {
                title: Some("Lynxby".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, move |_window, cx| {
            let config = config.clone();
            let store = store.clone();

            cx.new(move |cx| app::LynxbyApp::new(config, store, cx))
        })
        .expect("open the Lynxby window");

        cx.activate(true);
    });
}
