//! Submitted jobs (PSPU §7): the jobs socket's commands, the job's life
//! from submission to retention, and the control socket's view of it.

mod connection;
mod control;
mod deadlines;
mod lifecycle;
mod notify;
mod stop;
mod submit;
mod support;
mod view;
