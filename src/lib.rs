// FIXME: This should probably be made private at some point!
pub mod parser;

use std::iter::repeat;

use parser::{LateralChain, Modifications};
use petgraph::{stable_graph::NodeIndex, Graph};
use phf::phf_map;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

// FIXME: These masses need to be checked against the mass_calc databases Steph has vetted!
// Masses computed using https://mstools.epfl.ch/info/
// Checked against http://www.matrixscience.com/help/aa_help.html
// Used to update https://github.com/Mesnage-Org/rhizobium-pg-pipeline/blob/7f3a322624c027f5c42b796c6a1c0a1d7d81dbb0/Data/Constants/masses_table.csv
static RESIDUES: phf::Map<char, Moiety> = phf_map! {
    'A' => Moiety::new("A", "Alanine", dec!(71.037114)),
    'C' => Moiety::new("C", "Cysteine", dec!(103.009185)),
    'D' => Moiety::new("D", "Aspartic Acid", dec!(115.026943)),
    'E' => Moiety::new("E", "Glutamic Acid", dec!(129.042593)),
    'F' => Moiety::new("F", "Phenylalanine", dec!(147.068414)),
    'G' => Moiety::new("G", "Glycine", dec!(57.021464)),
    'H' => Moiety::new("H", "Histidine", dec!(137.058912)),
    'I' => Moiety::new("I", "Isoleucine", dec!(113.084064)),
    'J' => Moiety::new("J", "Diaminopimelic Acid", dec!(172.084792)),
    'K' => Moiety::new("K", "Lysine", dec!(128.094963)),
    'L' => Moiety::new("L", "Leucine", dec!(113.084064)),
    'M' => Moiety::new("M", "Methionine", dec!(131.040485)),
    'N' => Moiety::new("N", "Asparagine", dec!(114.042927)),
    'P' => Moiety::new("P", "Proline", dec!(97.052764)),
    'Q' => Moiety::new("Q", "Glutamine", dec!(128.058578)),
    'R' => Moiety::new("R", "Arginine", dec!(156.101111)),
    'S' => Moiety::new("S", "Serine", dec!(87.032028)),
    'T' => Moiety::new("T", "Threonine", dec!(101.047678)),
    'U' => Moiety::new("U", "Selenocysteine", dec!(150.953636)),
    'V' => Moiety::new("V", "Valine", dec!(99.068414)),
    'W' => Moiety::new("W", "Tryptophan", dec!(186.079313)),
    'Y' => Moiety::new("Y", "Tyrosine", dec!(163.063329)),
    'g' => Moiety::new("g", "GlcNAc", dec!(203.079373)),
    'm' => Moiety::new("m", "MurNAc", dec!(275.100502)),
    // These are placeholder residues, allowing modifications to entirely determine the residue mass
    'X' => Moiety::new("X", "Unknown Amino Acid", dec!(0.0)),
    'x' => Moiety::new("x", "Unknown Monosaccharide", dec!(0.0)),
};
// Used to update https://github.com/Mesnage-Org/rhizobium-pg-pipeline/blob/7f3a322624c027f5c42b796c6a1c0a1d7d81dbb0/Data/Constants/mods_table.csv
static MODIFICATIONS: phf::Map<&str, Moiety> = phf_map! {
    "+" => Moiety::new("+", "Proton", dec!(1.007276)),
    "H" => Moiety::new("H", "Hydrogen", dec!(1.007825)),
    "OH" => Moiety::new("OH", "Hydroxy", dec!(17.002740)),
    "Ac" => Moiety::new("Ac", "Acetyl", dec!(42.010565)),
};

// FIXME: Cache or memoise calculations of things like mass
#[derive(Clone, Debug)]
pub struct Peptidoglycan {
    name: String,
    graph: Graph<Residue, ()>,
}

impl Peptidoglycan {
    // FIXME: Replace `String` with a proper error type!
    // FIXME: Sopping code... Needs some DRYing!
    pub fn new(structure: &str) -> Result<Self, String> {
        // FIXME: Handle this error properly!
        let (_, (monomers, crosslinks)) = parser::multimer(structure).unwrap();
        let mut graph = Graph::new();
        for (glycan, peptide) in monomers {
            // Build the glycan chain
            let mut last_monosaccharide = graph.add_node(Residue::from(&glycan[0]));

            // Add H to the "N-terminal"
            graph[last_monosaccharide]
                .modifications
                .push(Modification::Add(MODIFICATIONS["H"]));

            // Add edges
            for monosaccharide in &glycan[1..] {
                let monosaccharide = graph.add_node(Residue::from(monosaccharide));
                graph.add_edge(last_monosaccharide, monosaccharide, ());
                last_monosaccharide = monosaccharide;
            }

            // Add H2 to the reduced end of the glycan chain
            // FIXME: Can GlcNAc be reduced too? Or only MurNAc?
            graph[last_monosaccharide]
                .modifications
                .extend(repeat(Modification::Add(MODIFICATIONS["H"])).take(2));

            if let Some(peptide) = peptide {
                let (abbr, modifications, lateral_chain) = &peptide[0];
                let mut last_amino_acid = graph.add_node(Residue::from((abbr, modifications)));

                build_lateral_chain(&mut graph, last_amino_acid, lateral_chain);

                // Join the glycan chain and peptide stem
                // FIXME: Needs to validate that this is a MurNAc
                graph.add_edge(last_monosaccharide, last_amino_acid, ());

                for (abbr, modifications, lateral_chain) in &peptide[1..] {
                    let amino_acid = graph.add_node(Residue::from((abbr, modifications)));
                    graph.add_edge(last_amino_acid, amino_acid, ());
                    last_amino_acid = amino_acid;
                    build_lateral_chain(&mut graph, last_amino_acid, lateral_chain);
                }

                // Add OH to the "C-terminal"
                graph[last_amino_acid]
                    .modifications
                    .push(Modification::Add(MODIFICATIONS["OH"]));
            } else {
                // Add OH to the "C-terminal"
                graph[last_monosaccharide]
                    .modifications
                    .push(Modification::Add(MODIFICATIONS["OH"]));
            }
        }

        // FIXME: Still very wet, needs breaking into more subfunctions!
        fn build_lateral_chain(
            graph: &mut Graph<Residue, ()>,
            last_amino_acid: NodeIndex,
            lateral_chain: &Option<LateralChain>,
        ) {
            if let Some(lateral_chain) = lateral_chain {
                let mut last_lateral_amino_acid = graph.add_node(Residue::from(&lateral_chain[0]));

                graph.add_edge(last_lateral_amino_acid, last_amino_acid, ());

                // Remove H from the branch-point residue
                graph[last_amino_acid]
                    .modifications
                    .push(Modification::Remove(MODIFICATIONS["H"]));

                for lateral_amino_acid in &lateral_chain[1..] {
                    let lateral_amino_acid = graph.add_node(Residue::from(lateral_amino_acid));
                    graph.add_edge(lateral_amino_acid, last_lateral_amino_acid, ());
                    last_lateral_amino_acid = lateral_amino_acid;
                }

                // Add H to the "N-terminal"
                graph[last_lateral_amino_acid]
                    .modifications
                    .push(Modification::Add(MODIFICATIONS["H"]));
            }
        }

        Ok(Self {
            name: structure.to_string(),
            graph,
        })
    }

    pub fn monoisotopic_mass(&self) -> Decimal {
        self.graph
            .raw_nodes()
            .iter()
            .map(|residue| residue.weight.monoisotopic_mass())
            .sum()
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Modification {
    Add(Moiety),
    Remove(Moiety),
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Residue {
    moiety: Moiety,
    modifications: Vec<Modification>,
}

impl Residue {
    pub fn monoisotopic_mass(&self) -> Decimal {
        self.modifications
            .iter()
            .fold(self.moiety.mass, |mass, modification| match modification {
                Modification::Add(m) => mass + m.mass,
                Modification::Remove(m) => mass - m.mass,
            })
    }
}

impl From<&(char, Option<Modifications>)> for Residue {
    fn from((abbr, modifications): &(char, Option<Modifications>)) -> Self {
        Residue::from((abbr, modifications))
    }
}

impl From<(&char, &Option<Modifications>)> for Residue {
    fn from((abbr, modifications): (&char, &Option<Modifications>)) -> Self {
        let moiety = RESIDUES[abbr];
        let modifications = modifications
            .iter()
            .flatten()
            // FIXME: This Modification conversion could be improved using `strum`?
            .map(|modification| match modification {
                parser::Modification::Add(abbr) => Modification::Add(MODIFICATIONS[abbr]),
                parser::Modification::Remove(abbr) => Modification::Remove(MODIFICATIONS[abbr]),
            })
            .collect();
        Self {
            moiety,
            modifications,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct Moiety {
    abbr: &'static str,
    name: &'static str,
    mass: Decimal,
}

impl Moiety {
    pub const fn new(abbr: &'static str, name: &'static str, mass: Decimal) -> Self {
        Self { abbr, name, mass }
    }
}

// FIXME: Keep writing messy code, then write tests, then refactor
#[cfg(test)]
mod tests {
    use std::error::Error;

    use petgraph::dot::{Config, Dot};

    use super::*;

    #[test]
    fn it_works() -> Result<(), Box<dyn Error>> {
        let pg = dbg!(Peptidoglycan::new("g(-Ac)m-AE[G]K[AKEAG]AA")?);
        println!("{:?}", Dot::with_config(&pg.graph, &[Config::EdgeNoLabel]));
        dbg!(dbg!(pg.monoisotopic_mass()).round_dp(4));
        panic!();
        Ok(())
    }
}
