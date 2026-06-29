use super::BoundaryError;

pub trait ConsoleSink {
    fn write_console(&mut self, message: &str) -> Result<(), BoundaryError>;
}
