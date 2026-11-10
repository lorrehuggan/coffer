fn main() {
    // `coffer --showcase` opens the component showcase (D2.5) instead of the
    // app. Argument parsing proper arrives with the CLI commands (M1).
    let showcase = std::env::args().skip(1).any(|arg| arg == "--showcase");

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            // Must be called before using any GPUI Kit components.
            gpui_kit::init(cx);
            coffer_app::theme::install(cx).expect("Failed to install the theme");

            if showcase {
                coffer_app::showcase::init(cx);
                coffer_app::showcase::open(cx).expect("Failed to open the showcase");
            } else {
                // The Transactions screen on fixture data (D5) until the
                // app shell arrives (M2).
                coffer_app::transactions::init(cx);
                coffer_app::transactions::open(cx).expect("Failed to open the window");
            }
        });
}
