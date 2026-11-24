//! A small builder for ONNX graphs assembled in code.
//!
//! Nodes and initializers are kept with owned names while the graph grows,
//! then lent to `onnx_rs` once to encode the model bytes ONNX Runtime loads.

use onnx_rs::ast::{
    Attribute, AttributeType, DataType, Dimension, Graph, Model, Node, OpType, OperatorSetId,
    TensorProto, TensorShape, TensorShapeDimension, TensorTypeProto, TypeProto, TypeValue,
    ValueInfo,
};

/// ONNX opset the built graphs target (LayerNormalization needs 17).
const OPSET: i64 = 17;

pub enum Attr {
    Int(&'static str, i64),
    Ints(&'static str, Vec<i64>),
    Float(&'static str, f32),
    /// A one-element float tensor (e.g. `ConstantOfShape`'s `value`).
    Scalar(&'static str, f32),
}

enum Data {
    /// Little-endian `f32` bytes, stored as `raw_data` (no copy when encoding).
    F32(Vec<u8>),
    I64(Vec<i64>),
}

struct Init {
    name: String,
    dims: Vec<i64>,
    data: Data,
}

struct NodeDef {
    op: &'static str,
    inputs: Vec<String>,
    output: String,
    attrs: Vec<Attr>,
}

/// A graph input or output: name, element type and shape (`None` = dynamic).
pub struct Io {
    pub name: &'static str,
    pub elem: DataType,
    pub dims: Vec<Option<i64>>,
}

#[derive(Default)]
pub struct Builder {
    nodes: Vec<NodeDef>,
    inits: Vec<Init>,
    next: usize,
}

impl Builder {
    fn fresh(&mut self, hint: &str) -> String {
        self.next += 1;
        format!("{hint}_{}", self.next)
    }

    /// Add a node with one output and return the output's name.
    pub fn op(&mut self, op: &'static str, inputs: &[&str], attrs: Vec<Attr>) -> String {
        let output = self.fresh(op);
        self.nodes.push(NodeDef {
            op,
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            output: output.clone(),
            attrs,
        });
        output
    }

    /// Like [`Builder::op`], writing to a chosen output name (a graph output).
    pub fn op_into(&mut self, output: &str, op: &'static str, inputs: &[&str], attrs: Vec<Attr>) {
        self.nodes.push(NodeDef {
            op,
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            output: output.to_string(),
            attrs,
        });
    }

    pub fn f32(&mut self, dims: Vec<i64>, data: Vec<f32>) -> String {
        debug_assert_eq!(dims.iter().product::<i64>() as usize, data.len());
        let name = self.fresh("w");
        self.inits.push(Init {
            name: name.clone(),
            dims,
            data: Data::F32(data.iter().flat_map(|v| v.to_le_bytes()).collect()),
        });
        name
    }

    pub fn i64(&mut self, dims: Vec<i64>, data: Vec<i64>) -> String {
        let name = self.fresh("c");
        self.inits.push(Init {
            name: name.clone(),
            dims,
            data: Data::I64(data),
        });
        name
    }

    /// A float scalar constant.
    pub fn scalar(&mut self, v: f32) -> String {
        self.f32(vec![], vec![v])
    }

    pub fn encode(&self, inputs: &[Io], outputs: &[Io]) -> Vec<u8> {
        fn value_info<'a>(io: &'a Io) -> ValueInfo<'a> {
            ValueInfo {
                name: io.name,
                r#type: Some(TypeProto {
                    value: Some(TypeValue::Tensor(TensorTypeProto {
                        elem_type: io.elem,
                        shape: Some(TensorShape {
                            dim: io
                                .dims
                                .iter()
                                .enumerate()
                                .map(|(i, d)| TensorShapeDimension {
                                    value: match d {
                                        Some(v) => Dimension::Value(*v),
                                        None => Dimension::Param(DYNAMIC[i.min(DYNAMIC.len() - 1)]),
                                    },
                                    denotation: "",
                                })
                                .collect(),
                        }),
                    })),
                    denotation: "",
                }),
                ..Default::default()
            }
        }
        fn node<'a>(n: &'a NodeDef) -> Node<'a> {
            Node {
                op_type: OpType::from(n.op),
                input: n.inputs.iter().map(String::as_str).collect(),
                output: vec![n.output.as_str()],
                attribute: n
                    .attrs
                    .iter()
                    .map(|a| match a {
                        Attr::Int(name, i) => Attribute {
                            name,
                            r#type: AttributeType::Int,
                            i: *i,
                            ..Default::default()
                        },
                        Attr::Ints(name, ints) => Attribute {
                            name,
                            r#type: AttributeType::Ints,
                            ints: ints.clone(),
                            ..Default::default()
                        },
                        Attr::Float(name, f) => Attribute {
                            name,
                            r#type: AttributeType::Float,
                            f: *f,
                            ..Default::default()
                        },
                        Attr::Scalar(name, f) => Attribute {
                            name,
                            r#type: AttributeType::Tensor,
                            t: Some(TensorProto::from_f32("", vec![1], vec![*f])),
                            ..Default::default()
                        },
                    })
                    .collect(),
                ..Default::default()
            }
        }
        let model = Model {
            ir_version: 8,
            opset_import: vec![OperatorSetId {
                domain: "",
                version: OPSET,
            }],
            producer_name: "latexsnipper",
            graph: Some(Graph {
                name: "main",
                node: self.nodes.iter().map(node).collect(),
                initializer: self
                    .inits
                    .iter()
                    .map(|t| match &t.data {
                        Data::F32(bytes) => {
                            TensorProto::from_raw(&t.name, t.dims.clone(), DataType::Float, bytes)
                        }
                        Data::I64(v) => TensorProto::from_i64(&t.name, t.dims.clone(), v.clone()),
                    })
                    .collect(),
                input: inputs.iter().map(value_info).collect(),
                output: outputs.iter().map(value_info).collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        onnx_rs::encode(&model)
    }
}

/// Names for dynamic dimensions, by position.
const DYNAMIC: [&str; 3] = ["batch", "sequence", "dim"];

#[cfg(test)]
mod tests {
    use super::*;
    use ort::session::Session;
    use ort::value::Tensor;

    #[test]
    fn built_graphs_run_in_onnx_runtime() {
        // y = relu(x * 2 - 1)
        let mut b = Builder::default();
        let two = b.scalar(2.0);
        let one = b.scalar(1.0);
        let m = b.op("Mul", &["x", &two], vec![]);
        let s = b.op("Sub", &[&m, &one], vec![]);
        b.op_into("y", "Relu", &[&s], vec![]);
        let io = |name| Io {
            name,
            elem: DataType::Float,
            dims: vec![None, Some(3)],
        };
        let bytes = b.encode(&[io("x")], &[io("y")]);
        let mut session = Session::builder()
            .unwrap()
            .commit_from_memory(&bytes)
            .unwrap();
        let x = Tensor::from_array((vec![1i64, 3], vec![0.0f32, 1.0, 2.0])).unwrap();
        let out = session.run(ort::inputs![x]).unwrap();
        let (_, y) = out[0].try_extract_tensor::<f32>().unwrap();
        assert_eq!(y, [0.0, 1.0, 3.0]);
    }
}
