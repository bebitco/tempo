use crate::consensus::MalachiteConsensusBuilder;
use reth_chainspec::ChainSpec;
use reth_node_builder::components::BasicPayloadServiceBuilder;
use reth_node_builder::{
    FullNodeTypes, Node, NodeComponentsBuilder, NodeTypes, components::ComponentsBuilder,
};
use reth_node_ethereum::node::{EthereumAddOns, EthereumNetworkBuilder, EthereumPoolBuilder};
use reth_trie_db::MerklePatriciaTrie;

/// Type configuration for a regular Malachite node.
#[derive(Debug, Clone, Default)]
pub struct MalachiteNode;

impl MalachiteNode {
    /// Create a new MalachiteNode
    pub fn new() -> Self {
        Self::default()
    }
}

impl NodeTypes for MalachiteNode {
    type Primitives = reth_ethereum_primitives::EthPrimitives;
    type ChainSpec = ChainSpec;
    type StateCommitment = MerklePatriciaTrie;
    type Storage = reth_provider::EthStorage;
    type Payload = reth_node_ethereum::EthEngineTypes;
}

impl<N> Node<N> for MalachiteNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        EthereumPoolBuilder,
        BasicPayloadServiceBuilder<reth_node_ethereum::EthereumPayloadBuilder>,
        EthereumNetworkBuilder,
        reth_node_ethereum::EthereumExecutorBuilder,
        MalachiteConsensusBuilder,
    >;

    type AddOns = EthereumAddOns<
        reth_node_builder::NodeAdapter<
            N,
            <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components,
        >,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        ComponentsBuilder::default()
            .node_types::<N>()
            .pool(EthereumPoolBuilder::default())
            .executor(reth_node_ethereum::EthereumExecutorBuilder::default())
            .payload(BasicPayloadServiceBuilder::default())
            .network(EthereumNetworkBuilder::default())
            .consensus(MalachiteConsensusBuilder::new())
    }

    fn add_ons(&self) -> Self::AddOns {
        EthereumAddOns::default()
    }
}

