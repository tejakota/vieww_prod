// live.rs — vieww Studio's Live Preview reads this file.
//
// It is NOT compiled and not part of your app: the studio parses it and draws
// it, which is why it updates as you type and why it cannot run any logic. Use
// it to sketch a flow — the screens, what is on them, and the way between them.
// Press Render to see real code instead; that one compiles.
//
// Soma's flow: one real screen (src/screens/home.rs) whose floating card has
// three states — resting, results, answer. The sketch lays them out as
// screens so the flow can be read and changed before it is built.
//
// Delete this file and the Live Preview turns off.

live! {
    screen "Body" {
        title "SOMA"
        text "11,043 health topics · offline. Drag to turn, pinch to zoom, tap a region to open it."
        // The real screen, exactly as it last rendered. Press Render on
        // src/screens/home.rs once to fill this in.
        mount "src/screens/home.rs"
        row "chest pain" "Suggestion: starts a search"
        row "tired all the time" "Suggestion"
        row "kidney stones" "Suggestion"
        button "Ask: kidney stones" -> "Results"
    }

    screen "Results" {
        title "24 answers, best first"
        text "The camera flies to the leading organ while you type; the card rides to the other end."
        row "Kidney Stones" "Condition · Kidneys · TEXT · MEANING"
        row "Kidneys" "Body · Kidneys · MEANING"
        row "Bladder Stones" "Condition · Bladder · NEARBY"
        text "Information only, not medical advice."
        button "Open Kidney Stones" -> "Answer"
        button "Clear" -> "Body"
    }

    screen "Answer" {
        title "Kidney Stones"
        text "Condition · Kidneys. The 3D kidneys are framed clear of this card; the callout cites the words that matched."
        text "Matched: Kidney stones are hard deposits that form in your kidneys…"
        row "Kidneys" "Nearby in the body"
        row "Bladder" "Nearby in the body"
        text "Source: my.clevelandclinic.org"
        button "Back" -> "Results"
    }

    screen "Emergency" {
        title "This could be an emergency"
        text "Shown above results and answers for red-flag questions (chest pain, stroke signs, trouble breathing, self-harm)."
        text "Call your local emergency number now (112 in India and the EU, 911 in the US, 999 in the UK)."
        button "Back to results" -> "Results"
    }

    nav "Body" "Results" "Answer" "Emergency"
}
