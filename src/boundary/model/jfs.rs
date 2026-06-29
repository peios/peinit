use super::error::BoundaryError;

pub trait JfsDevice {
    fn open_and_register(&mut self) -> Result<(), BoundaryError>;
}
