#[derive(Debug, Clone)]
pub enum ChannelHop {
    Vertex(super::vertex::VertexId),
}
