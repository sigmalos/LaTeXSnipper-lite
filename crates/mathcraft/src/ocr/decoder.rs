use anyhow::{bail, Context, Result};
use onnx_rs::ast::{
    Attribute, AttributeType, Dimension, Graph, Node, OpType, TensorProto, TypeValue,
};

const INDEX: &str = "latexsnipper_last_index";

/// `Gather(input, [-1], axis=1)`: keep only the last sequence position.
fn last_position<'a>(input: &'a str, output: &'a str) -> Node<'a> {
    Node {
        op_type: OpType::Gather,
        input: vec![input, INDEX],
        output: vec![output],
        attribute: vec![Attribute {
            name: "axis",
            r#type: AttributeType::Int,
            i: 1,
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// Return only the last sequence position's vocabulary logits during greedy decoding.
///
/// When the graph ends in a plain `MatMul` projection (the FP32 model), the last
/// hidden state is selected before it, so the projection runs on one position.
/// Any other ending (e.g. the dynamically quantized Int8 model, whose projection
/// is `DynamicQuantizeLinear → MatMulInteger → Cast → Mul`) selects the last
/// position of the logits instead, which leaves its numerics untouched.
pub fn next_token_graph(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut model = onnx_rs::parse(bytes).context("invalid formula decoder ONNX")?;
    let graph = model
        .graph
        .as_mut()
        .context("formula decoder has no graph")?;
    let [output] = graph.output.as_mut_slice() else {
        bail!("expected one formula decoder output");
    };
    let Some(TypeValue::Tensor(tensor)) = output.r#type.as_mut().and_then(|t| t.value.as_mut())
    else {
        bail!("expected formula decoder logits tensor");
    };
    let shape = tensor
        .shape
        .as_mut()
        .context("missing formula logits shape")?;
    let [_, _, vocab] = shape.dim.as_slice() else {
        bail!("expected formula logits with batch, sequence and vocabulary dimensions");
    };
    let vocab = vocab.value.clone();
    shape.dim.remove(1);
    let logits = output.name;
    if !select_before_projection(graph, logits, &vocab) {
        select_logits(graph, logits)?;
    }
    graph
        .initializer
        .push(TensorProto::from_i64(INDEX, vec![], vec![-1]));
    Ok(onnx_rs::encode(&model))
}

fn select_before_projection(graph: &mut Graph<'_>, logits: &str, vocab: &Dimension) -> bool {
    const SELECTED: &str = "latexsnipper_last_hidden";
    let Some(projection) = graph.node.last() else {
        return false;
    };
    let &[hidden, weights] = projection.input.as_slice() else {
        return false;
    };
    let plain_matmul = projection.op_type == OpType::MatMul
        && projection.domain.is_empty()
        && projection.output == [logits]
        && graph.initializer.iter().any(|t| {
            t.name() == weights
                && matches!(t.dims(), [_, width] if *vocab == Dimension::Value(*width))
        });
    if !plain_matmul {
        return false;
    }
    let gather = last_position(hidden, SELECTED);
    let at = graph.node.len() - 1;
    graph.node[at].input[0] = SELECTED;
    graph.node.insert(at, gather);
    true
}

fn select_logits(graph: &mut Graph<'_>, logits: &str) -> Result<()> {
    const FULL: &str = "latexsnipper_all_logits";
    let producer = graph
        .node
        .iter_mut()
        .rev()
        .find_map(|n| n.output.iter_mut().find(|o| **o == logits))
        .context("no node produces the formula logits")?;
    let logits = *producer;
    *producer = FULL;
    graph.node.push(last_position(FULL, logits));
    Ok(())
}
