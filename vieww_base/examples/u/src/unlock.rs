//! The lock screen — the door the biometric setting closes.
//!
//! Pushed over home at startup when the lock is on, and popped only by a
//! successful check. The system's sheet is the prompt; this screen is what
//! stands behind it: the companion's sigil breathing in the dark, the one
//! primary action, and — after a check settles — either home, one tap
//! later, or the honest word for what the sheet said: not recognised,
//! cancelled, nothing enrolled.
//!
//! # The task lives in the widget, not the state
//!
//! The [`Task`] is a field of the screen's widget struct — `Rc<RefCell<…>>`
//! so every rebuilt clone shares it — rather than of `AppState`, because a
//! prompt belongs to the door that opened it and nothing else in the app
//! cares how it ends. The build polls it (the screen reads the clock, so a
//! build runs every tick; `Task::poll` is the channel check the design
//! asks for) and writes only signals — the nav push stays in the tap
//! handler, where every other navigation in the app already lives.
//!
//! # The back gesture is deliberately absent
//!
//! Every other screen here leaves on a swipe; this one must not. The only
//! way out is the check succeeding, and the system's own back button
//! finishing the activity — which closes the app, and a closed app is as
//! locked as an open one.

use std::cell::RefCell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::capability::{BiometricPrompt, BiometricStrength, Biometrics};
use vieww_foundation::service::ServiceError;
use vieww_foundation::task::{AsyncValue, Task};

use crate::neurons;
use crate::palette::{CARD, EDGE, INK, MUTED};
use crate::stage;
use crate::{take, AppState, Shared};

/// The line the screen says under the action. `None` is the resting line;
/// everything else is what the last check settled into.
const RESTING: &str = "Your fingerprint or face opens U";
const ASKING: &str = "Waiting on the system sheet…";
const UNLOCKED: &str = "Unlocked";
const NOT_RECOGNISED: &str = "Not recognised — try again";
const CANCELLED: &str = "Cancelled — try again when ready";
const COULD_NOT_RUN: &str = "The sheet could not run — try again";

#[derive(Debug)]
pub struct UnlockScreen {
    pub state: AppState,
    pub shared: Shared,
    /// The check in flight, if any. Shared across this screen's rebuilds —
    /// the widget struct is rebuilt, the task is not.
    pub auth: Rc<RefCell<Option<Task<BiometricStrength, ServiceError>>>>,
}

/// The route, standing as the navigator's root while the app is locked.
#[must_use]
pub fn route(shared: Shared) -> Route {
    Route::new(
        "unlock",
        Rc::new(move |_| {
            UnlockScreen {
                state: take(&shared),
                shared: shared.clone(),
                auth: Rc::new(RefCell::new(None)),
            }
            .into()
        }),
    )
}

/// Whether the lock stands: the setting is on *and* this device has
/// something to check — a lock on a sensorless phone would be a door with
/// no handle, and the honest reading of the setting there is "no biometric
/// to lock with".
#[must_use]
pub fn locked(state: &AppState) -> bool {
    if !state.biometric.get() {
        return false;
    }
    state
        .services
        .get::<dyn Biometrics>()
        .is_some_and(|capability| !capability.available().is_empty())
}

#[widget]
impl UnlockScreen {
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();
        let t = state.time.get();

        // Poll the in-flight check: this build runs every clock tick, so
        // the result is noticed the moment its frame lands. The outcome is
        // written to signals — the only effect a build may have — and the
        // navigation stays in the tap handler below.
        if let Some(task) = self.auth.borrow_mut().as_mut() {
            if task.poll() {
                match task.value() {
                    AsyncValue::Ready(_) => {
                        state.unlocked.set(true);
                        state.unlock_status.set(Some(UNLOCKED.to_owned()));
                    }
                    AsyncValue::Failed(ServiceError::Denied(why)) => {
                        state.unlock_status.set(Some(
                            if why.contains("cancelled") {
                                CANCELLED
                            } else {
                                NOT_RECOGNISED
                            }
                            .to_owned(),
                        ));
                    }
                    AsyncValue::Failed(_) | AsyncValue::Lost => {
                        state.unlock_status.set(Some(COULD_NOT_RUN.to_owned()));
                    }
                    AsyncValue::Pending => state.unlock_status.set(Some(ASKING.to_owned())),
                }
                *self.auth.borrow_mut() = None;
            }
        }

        let unlocked = state.unlocked.get();
        let line = state
            .unlock_status
            .get()
            .unwrap_or_else(|| RESTING.to_owned());

        let shared_for_tap = self.shared.clone();
        let auth = self.auth.clone();
        let button = Pressable::themed(ctx, move |press| {
            let sink = 1.0 - press * 0.04;
            SizedBox::height(52.0)
                .child(
                    Align::new(Alignment::CENTER).child(
                        Transformed::new(Transform::scale_around(
                            Offset::new(186.0, 26.0),
                            sink,
                            sink,
                        ))
                        .child(
                            Container::new()
                                .decoration(
                                    BoxDecoration::new()
                                        .gradient(crate::palette::action_gradient())
                                        .radius(26.0)
                                        .shadow(Shadow::new(
                                            Color::rgba(0, 0, 0, 122),
                                            Offset::new(0.0, 6.0),
                                            9.0,
                                        )),
                                )
                                .width(372.0)
                                .height(52.0)
                                .alignment(Alignment::CENTER)
                                .child(
                                    Text::new(if unlocked { "Continue" } else { "Unlock" })
                                        .style(
                                            TextStyle::new(15.0)
                                                .weight(FontWeight::Medium)
                                                .color(Color::rgb(255, 255, 255)),
                                        )
                                        .align(TextAlign::Center),
                                ),
                        ),
                    ),
                )
                .into()
        })
        .on_tap(move || {
            let state = take(&shared_for_tap);
            if state.unlocked.get() {
                // The check passed; the door opens. Home is beneath.
                state.nav.pop();
                return;
            }
            if auth.borrow().is_some() {
                // A prompt is already up; one at a time, as the shim
                // enforces anyway.
                return;
            }
            // Ask the gate, then run the prompt. The gate's `access` is the
            // framework's own shape — the grant proves the permission was
            // resolved, and the task carries the wait.
            let Some(gate) = state.services.gate::<dyn Biometrics>() else {
                return;
            };
            let auth_for_ask = auth.clone();
            let state_for_refuse = state.clone();
            gate.access(
                move |capability, grant| {
                    let prompt = BiometricPrompt {
                        title: "Unlock U".to_owned(),
                        subtitle: Some("The companion is behind this door".to_owned()),
                        cancel_label: "Not now".to_owned(),
                        // The device credential is the plan's own answer to
                        // "no biometric enrolled": PIN and pattern stand in,
                        // so the lock never bricks a person out of their
                        // app.
                        allow_device_credential: true,
                    };
                    *auth_for_ask.borrow_mut() = Some(capability.authenticate(&prompt, grant));
                },
                move |_refused: vieww_foundation::permission::PermissionState| {
                    // USE_BIOMETRIC is a normal permission; a refusal here
                    // means the manifest lost it, and the honest word is
                    // the same one a broken sheet gets.
                    state_for_refuse
                        .unlock_status
                        .set(Some(COULD_NOT_RUN.to_owned()));
                },
            );
        });

        let screen = Stack::new()
            .fit(StackFit::Expand)
            .push(Positioned::fill().child(neurons::atmosphere(t, 0.4)))
            .push(place(0.0, top_of(272.0, 40.0)).width(420.0).child(
                Text::new("U")
                    .style(
                        TextStyle::new(40.0)
                            .weight(FontWeight::Bold)
                            .letter_spacing(-2.5)
                            .color(Color::rgb(241, 239, 255)),
                    )
                    .align(TextAlign::Center),
            ))
            .push(place(0.0, top_of(287.0, 8.5)).width(420.0).child(
                Text::new("PERSONA COMPANION")
                    .style(
                        TextStyle::new(8.5)
                            .weight(FontWeight::Medium)
                            .letter_spacing(2.6)
                            .color(Color::rgb(155, 139, 220)),
                    )
                    .align(TextAlign::Center),
            ))
            .push(place(185.0, 296.0).child(neurons::sigil(50.0, t, 0.0)))
            .push(place(0.0, top_of(372.0, 17.0)).width(420.0).child(
                Text::new("Locked")
                    .style(
                        TextStyle::new(17.0)
                            .weight(FontWeight::Medium)
                            .color(INK),
                    )
                    .align(TextAlign::Center),
            ))
            .push(place(24.0, 396.0).child(
                SizedBox::from_size(Size::new(372.0, 44.0)).child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .color(CARD)
                                .radius(14.0)
                                .border(Border::new(EDGE, 1.0)),
                        )
                        .alignment(Alignment::CENTER)
                        .padding(EdgeInsets::symmetric(12.0, 0.0))
                        .child(
                            Text::new(line)
                                .style(TextStyle::new(11.0).color(MUTED))
                                .align(TextAlign::Center),
                        ),
                ),
            ))
            .push(place(24.0, 452.0).child(button))
            .push(place(0.0, top_of(512.0, 9.0)).width(420.0).child(
                Text::new("U  ·  ON-DEVICE PERSONA")
                    .style(
                        TextStyle::new(9.0)
                            .letter_spacing(0.6)
                            .color(Color::rgb(94, 109, 132)),
                    )
                    .align(TextAlign::Center),
            ));

        stage::screen(screen)
    }
}

/// Where a glyph's baseline sits below its line box's top — Inter's own
/// ascent, the same constant the other screens place text by.
const ASCENT: f32 = 0.969;

/// A text box top for a baseline at `baseline`.
fn top_of(baseline: f32, size: f32) -> f32 {
    baseline - ASCENT * size
}

/// A `Positioned` at the design's own coordinates.
fn place(left: f32, top: f32) -> Positioned {
    Positioned::new().left(left).top(top)
}
