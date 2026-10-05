//! Device glue (excluded from coverage, see CLAUDE.md): cpal and arboard calls only. Every decision they
//! need lives in a tested module (`audio_capture`, `clipboard`).
pub mod clipboard;
pub mod microphone;
