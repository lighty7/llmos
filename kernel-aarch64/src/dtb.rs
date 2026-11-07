//! Flattened device tree (DTB) parser.
//!
//! ARM boards describe their hardware in a device tree that the firmware or
//! QEMU passes to the kernel: memory ranges, the serial port, the initrd
//! location, CPUs, and every peripheral with its `compatible` string. It is
//! the ARM counterpart of the ACPI tables and PCI scan the x86-64 kernel
//! uses.
//!
//! Format: Devicetree Specification v0.4, chapter 5 "Flattened Devicetree
//! (DTB) Format". Only what llmos needs is parsed; `ranges` translation is
//! applied for one level of bus nesting (enough for the Raspberry Pi `/soc`).

use alloc::string::String;
use alloc::vec::Vec;

const MAGIC: u32 = 0xD00D_FEED;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;
const END: u32 = 9;

/// One node with the properties llmos cares about.
#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Full path, e.g. `/soc/serial@7e201000`.
    pub path: String,
    /// `compatible` strings.
    pub compatible: Vec<String>,
    /// Raw `reg` property.
    pub reg: Vec<u8>,
    /// Raw `ranges` property.
    pub ranges: Vec<u8>,
    /// `device_type` property.
    pub device_type: String,
    /// `#address-cells` declared by this node for its children.
    pub address_cells: Option<u32>,
    /// `#size-cells` declared by this node for its children.
    pub size_cells: Option<u32>,
    /// Index of the parent node.
    pub parent: Option<usize>,
    /// Other small properties: `(name, value)`.
    pub props: Vec<(String, Vec<u8>)>,
}

/// A parsed device tree.
#[derive(Debug, Clone, Default)]
pub struct DeviceTree {
    /// All nodes in document order (index 0 is the root).
    pub nodes: Vec<Node>,
    /// Total size of the blob in bytes.
    pub size: usize,
}

fn be32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// Reads a big-endian number of `cells` 32-bit cells.
fn cells(b: &[u8], cells: u32) -> Option<u64> {
    match cells {
        1 => be32(b, 0).map(u64::from),
        2 => Some((be32(b, 0)? as u64) << 32 | be32(b, 4)? as u64),
        _ => None,
    }
}

fn c_str(b: &[u8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..end]).unwrap_or("")
}

impl DeviceTree {
    /// Parses the blob at physical address `addr`.
    ///
    /// # Safety
    ///
    /// `addr` must point to a device tree blob in readable memory.
    pub unsafe fn parse(addr: usize) -> Option<Self> {
        // SAFETY: the header is 40 bytes; the caller guarantees a blob.
        let header = unsafe { core::slice::from_raw_parts(addr as *const u8, 40) };
        if be32(header, 0)? != MAGIC {
            return None;
        }
        let size = be32(header, 4)? as usize;
        // SAFETY: `totalsize` covers the whole blob.
        let blob = unsafe { core::slice::from_raw_parts(addr as *const u8, size) };
        let off_struct = be32(header, 8)? as usize;
        let off_strings = be32(header, 12)? as usize;

        let mut nodes: Vec<Node> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut pos = off_struct;
        loop {
            let token = be32(blob, pos)?;
            pos += 4;
            match token {
                BEGIN_NODE => {
                    let name = c_str(&blob[pos..]);
                    pos = (pos + name.len() + 1).div_ceil(4) * 4;
                    let parent = stack.last().copied();
                    let path = match parent {
                        None => String::from("/"),
                        Some(p) if nodes[p].path == "/" => alloc::format!("/{name}"),
                        Some(p) => alloc::format!("{}/{name}", nodes[p].path),
                    };
                    nodes.push(Node {
                        path,
                        parent,
                        ..Node::default()
                    });
                    stack.push(nodes.len() - 1);
                }
                END_NODE => {
                    stack.pop();
                }
                PROP => {
                    let len = be32(blob, pos)? as usize;
                    let name_off = be32(blob, pos + 4)? as usize;
                    let value = blob.get(pos + 8..pos + 8 + len)?;
                    pos = (pos + 8 + len).div_ceil(4) * 4;
                    let name = c_str(blob.get(off_strings + name_off..)?);
                    let node = nodes.get_mut(*stack.last()?)?;
                    match name {
                        "compatible" => {
                            node.compatible = value
                                .split(|&c| c == 0)
                                .filter(|s| !s.is_empty())
                                .map(|s| String::from(core::str::from_utf8(s).unwrap_or("")))
                                .collect();
                        }
                        "reg" => node.reg = value.to_vec(),
                        "ranges" => node.ranges = value.to_vec(),
                        "device_type" => node.device_type = c_str(value).into(),
                        "#address-cells" => node.address_cells = be32(value, 0),
                        "#size-cells" => node.size_cells = be32(value, 0),
                        _ if value.len() <= 256 => node.props.push((name.into(), value.to_vec())),
                        _ => {}
                    }
                }
                NOP => {}
                END => break,
                _ => return None,
            }
        }
        Some(Self { nodes, size })
    }

    /// Looks up a node by exact path.
    pub fn find(&self, path: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.path == path)
    }

    /// Returns a raw property of a node.
    pub fn prop<'a>(&self, node: &'a Node, name: &str) -> Option<&'a [u8]> {
        node.props
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }

    /// `(#address-cells, #size-cells)` that apply to the children of `index`.
    fn child_cells(&self, index: usize) -> (u32, u32) {
        let n = &self.nodes[index];
        (n.address_cells.unwrap_or(2), n.size_cells.unwrap_or(1))
    }

    /// Decodes the `reg` property of node `index` into `(address, size)`
    /// pairs, translated to CPU physical addresses through the parent's
    /// `ranges` (one level).
    pub fn regs(&self, index: usize) -> Vec<(u64, u64)> {
        let node = &self.nodes[index];
        let Some(parent) = node.parent else {
            return Vec::new();
        };
        let (ac, sc) = self.child_cells(parent);
        let stride = 4 * (ac + sc) as usize;
        let mut out = Vec::new();
        if stride == 0 {
            return out;
        }
        for entry in node.reg.chunks_exact(stride) {
            let (Some(addr), Some(size)) = (cells(entry, ac), cells(&entry[4 * ac as usize..], sc))
            else {
                continue;
            };
            out.push((self.translate(parent, addr), size));
        }
        out
    }

    /// Translates a child-bus address of `bus` into its parent's space.
    fn translate(&self, bus: usize, addr: u64) -> u64 {
        let node = &self.nodes[bus];
        let Some(grandparent) = node.parent else {
            return addr;
        };
        if node.ranges.is_empty() {
            return addr; // Empty `ranges`: identity mapping.
        }
        let (child_ac, child_sc) = self.child_cells(bus);
        let (parent_ac, _) = self.child_cells(grandparent);
        let stride = 4 * (child_ac + parent_ac + child_sc) as usize;
        for r in node.ranges.chunks_exact(stride.max(4)) {
            let child = cells(r, child_ac).unwrap_or(0);
            let parent = cells(&r[4 * child_ac as usize..], parent_ac).unwrap_or(0);
            let size = cells(&r[4 * (child_ac + parent_ac) as usize..], child_sc).unwrap_or(0);
            if addr >= child && addr < child + size {
                return addr - child + parent;
            }
        }
        addr
    }

    /// RAM ranges from every `memory` node.
    pub fn memory(&self) -> Vec<(u64, u64)> {
        (0..self.nodes.len())
            .filter(|&i| {
                self.nodes[i].device_type == "memory" || self.nodes[i].path.starts_with("/memory")
            })
            .flat_map(|i| self.regs(i))
            .filter(|&(_, size)| size > 0)
            .collect()
    }

    /// The initrd range from `/chosen`, if the bootloader provided one.
    pub fn initrd(&self) -> Option<(u64, u64)> {
        let chosen = self.find("/chosen")?;
        let read = |name| {
            let v = self.prop(chosen, name)?;
            cells(v, (v.len() / 4) as u32)
        };
        let (start, end) = (read("linux,initrd-start")?, read("linux,initrd-end")?);
        (end > start).then_some((start, end))
    }

    /// Index of the first node whose `compatible` contains `needle`.
    pub fn find_compatible(&self, needle: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.compatible.iter().any(|c| c == needle))
    }

    /// Number of CPU nodes.
    pub fn cpu_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.device_type == "cpu").count()
    }

    /// Board name from the root `model` (or `compatible`) property.
    pub fn model(&self) -> String {
        let root = &self.nodes[0];
        self.prop(root, "model")
            .map(|m| String::from(c_str(m)))
            .or_else(|| root.compatible.first().cloned())
            .unwrap_or_default()
    }

    /// PSCI calling method (`"hvc"` or `"smc"`), if the board has PSCI.
    pub fn psci_method(&self) -> Option<String> {
        let idx = self
            .find_compatible("arm,psci-1.0")
            .or_else(|| self.find_compatible("arm,psci-0.2"))?;
        self.prop(&self.nodes[idx], "method")
            .map(|m| String::from(c_str(m)))
    }
}
