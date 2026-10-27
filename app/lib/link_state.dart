// What the link is doing, in the four words the whole app agrees on.
//
// In a file of its own because both the screen and the session state need it, and a screen that
// imports main.dart to find out what "connected" means is a screen that cannot be rendered on its
// own in a test.
//
// The distinction that matters: CONNECTED is the PC's own reply, never "we pressed Start".
// CONNECTING is a session that has heard nothing *yet*; NO ANSWER is one that has waited long
// enough that silence is a verdict. The app once showed LIVE while its audio went nowhere, and
// these four states are the fix.
// NOT PLAYING is the fifth and the newest, and it exists because CONNECTED was still claiming too
// much. A receiver answers any packet it can PARSE -- including audio it then throws away. On
// 2026-10-26 a PC ran a version that predated Opus: it replied to every packet, the phone went
// green, and not one word was ever heard. "The PC is answering" and "the PC is playing you" are
// different facts, and the reply carries both.
enum LinkState { idle, connecting, connected, notPlaying, noAnswer }
