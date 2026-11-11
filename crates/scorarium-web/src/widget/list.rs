/// The edits that a form makes to one list
pub(crate) struct Edits<R, V> {
    /// The addresses of the stored values to remove
    pub remove: Vec<R>,
    /// The values to add
    pub add: Vec<V>,
}
