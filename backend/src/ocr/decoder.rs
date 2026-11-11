use anyhow::{bail, Context, Result};
use onnx_rs::ast::{Attribute, AttributeType, Dimension, Node, OpType, TensorProto, TypeValue};

/// Project only the last sequence position to vocabulary logits during greedy decoding.
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
    let projection = graph
        .node
        .last_mut()
        .context("missing formula output projection")?;
    let [hidden, weights] = projection.input.as_mut_slice() else {
        bail!("expected two formula projection inputs");
    };
    if projection.op_type != OpType::MatMul
        || !projection.domain.is_empty()
        || projection.output != [output.name]
        || !graph.initializer.iter().any(|t| {
            t.name() == *weights
                && matches!(t.dims(), [_, width] if vocab.value == Dimension::Value(*width))
        })
    {
        bail!("unexpected formula output projection");
    }

    const INDEX: &str = "latexsnipper_last_index";
    const SELECTED: &str = "latexsnipper_last_hidden";
    let gather = Node {
        op_type: OpType::Gather,
        input: vec![*hidden, INDEX],
        output: vec![SELECTED],
        attribute: vec![Attribute {
            name: "axis",
            r#type: AttributeType::Int,
            i: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    *hidden = SELECTED;
    graph.node.insert(graph.node.len() - 1, gather);
    graph
        .initializer
        .push(TensorProto::from_i64(INDEX, vec![], vec![-1]));
    shape.dim.remove(1);
    Ok(onnx_rs::encode(&model))
}
