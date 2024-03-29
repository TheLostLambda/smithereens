use std::iter;

use rust_decimal::Decimal;

use crate::{
    Charge, Charged, FunctionalGroup, GroupState, Id, Massive, Mz, PolychemError, Residue, Result,
};

use super::polymer_database::{PolymerDatabase, ResidueDescription};

impl<'a, 'p> Residue<'a, 'p> {
    pub fn new(db: &'p PolymerDatabase<'a>, abbr: impl AsRef<str>, id: Id) -> Result<Self> {
        let abbr = abbr.as_ref();
        let (
            abbr,
            ResidueDescription {
                name,
                composition,
                functional_groups,
            },
        ) = db
            .residues
            .get_key_value(abbr)
            .ok_or_else(|| PolychemError::residue_lookup(abbr))?;
        let functional_groups = functional_groups
            .iter()
            .map(|fg| (fg.into(), GroupState::default()))
            .collect();
        Ok(Self {
            id,
            abbr,
            name,
            composition,
            functional_groups,
            offset_modifications: Vec::new(),
        })
    }

    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    #[must_use]
    pub const fn abbr(&self) -> &'p str {
        self.abbr
    }

    #[must_use]
    pub const fn name(&self) -> &'p str {
        self.name
    }

    pub fn group_state(
        &self,
        functional_group: &FunctionalGroup<'p>,
    ) -> Result<&GroupState<'a, 'p>> {
        self.functional_groups.get(functional_group).ok_or_else(|| {
            PolychemError::group_lookup(*functional_group, self.name, self.abbr).into()
        })
    }

    pub fn group_state_mut(
        &mut self,
        functional_group: &FunctionalGroup<'p>,
    ) -> Result<&mut GroupState<'a, 'p>> {
        self.functional_groups
            .get_mut(functional_group)
            .ok_or_else(|| {
                PolychemError::group_lookup(*functional_group, self.name, self.abbr).into()
            })
    }
}

// NOTE: This needs to be a macro, since all of the Massive::monoisotopic_mass calls will actually have different types!
// The other way to do this would be to use trait-objects, but this avoids that overhead
macro_rules! sum_parts {
    ($self:expr, $accessor:path) => {{
        let composition = iter::once($accessor($self.composition));
        let functional_groups = $self.functional_groups.values().filter_map(|gs| match gs {
            GroupState::Modified(m) => Some($accessor(m)),
            GroupState::Donor(b) => Some($accessor(b)),
            _ => None,
        });
        let offset_mods = $self.offset_modifications.iter().map($accessor);

        composition
            .chain(functional_groups)
            .chain(offset_mods)
            .sum()
    }};
}

impl Massive for Residue<'_, '_> {
    fn monoisotopic_mass(&self) -> Decimal {
        sum_parts!(self, Massive::monoisotopic_mass)
    }

    fn average_mass(&self) -> Decimal {
        sum_parts!(self, Massive::average_mass)
    }
}

impl Charged for Residue<'_, '_> {
    fn charge(&self) -> Charge {
        sum_parts!(self, Charged::charge)
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use once_cell::sync::Lazy;
    use rust_decimal_macros::dec;

    use crate::{
        testing_tools::assert_miette_snapshot, AtomicDatabase, Bond, BondTarget, Modification,
        NamedMod, OffsetKind, OffsetMod,
    };

    use super::*;

    static ATOMIC_DB: Lazy<AtomicDatabase> = Lazy::new(AtomicDatabase::default);

    static POLYMER_DB: Lazy<PolymerDatabase> = Lazy::new(|| {
        PolymerDatabase::new(
            &ATOMIC_DB,
            "polymer_database.kdl",
            include_str!("../../tests/data/polymer_database.kdl"),
        )
        .unwrap()
    });

    #[test]
    fn errors() {
        let sucrose = Residue::new(&POLYMER_DB, "s", 0);
        assert_miette_snapshot!(sucrose);
        let super_amino = Residue::new(&POLYMER_DB, "Sa", 0);
        assert_miette_snapshot!(super_amino);
    }

    #[test]
    fn id() {
        let alanine = Residue::new(&POLYMER_DB, "A", 0).unwrap();
        assert_eq!(alanine.id(), 0);
        let alanine = Residue::new(&POLYMER_DB, "A", 42).unwrap();
        assert_eq!(alanine.id(), 42);
        let alanine = Residue::new(&POLYMER_DB, "A", 1).unwrap();
        assert_eq!(alanine.id(), 1);
    }

    static N_TERMINAL: FunctionalGroup = FunctionalGroup::new("Amino", "N-Terminal");

    static C_TERMINAL: FunctionalGroup = FunctionalGroup::new("Carboxyl", "C-Terminal");

    #[test]
    fn group_state() {
        let lysine = Residue::new(&POLYMER_DB, "K", 0).unwrap();
        let alanine = Residue::new(&POLYMER_DB, "A", 0).unwrap();
        let sidechain_amino = FunctionalGroup::new("Amino", "Sidechain");

        // Sucessfully lookup functional groups that exist
        assert!(lysine.group_state(&N_TERMINAL).is_ok());
        assert!(lysine.group_state(&C_TERMINAL).is_ok());
        assert!(lysine.group_state(&sidechain_amino).is_ok());

        assert!(alanine.group_state(&N_TERMINAL).is_ok());
        assert!(alanine.group_state(&C_TERMINAL).is_ok());

        // Fail to lookup functional groups that don't exist
        assert_miette_snapshot!(alanine.group_state(&sidechain_amino));
    }

    #[test]
    fn group_state_mut() {
        let mut lysine = Residue::new(&POLYMER_DB, "K", 0).unwrap();
        let mut alanine = Residue::new(&POLYMER_DB, "A", 0).unwrap();
        let sidechain_amino = FunctionalGroup::new("Amino", "Sidechain");

        // Sucessfully lookup functional groups that exist
        assert!(lysine.group_state_mut(&N_TERMINAL).is_ok());
        assert!(lysine.group_state_mut(&C_TERMINAL).is_ok());
        assert!(lysine.group_state_mut(&sidechain_amino).is_ok());

        assert!(alanine.group_state_mut(&N_TERMINAL).is_ok());
        assert!(alanine.group_state_mut(&C_TERMINAL).is_ok());

        // Fail to lookup functional groups that don't exist
        assert_miette_snapshot!(alanine.group_state_mut(&sidechain_amino));
    }

    static RESIDUE_SERIES: Lazy<Vec<Residue<'static, 'static>>> = Lazy::new(|| {
        let mut snapshots = Vec::new();

        let mut alanine = Residue::new(&POLYMER_DB, "A", 0).unwrap();
        snapshots.push(alanine.clone());

        // Add a water-loss offset modification
        let water_loss = Modification::new(
            1,
            OffsetMod::new(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap(),
        );
        alanine.offset_modifications.push(water_loss);
        snapshots.push(alanine.clone());

        // Add an amidation named modification to the C-terminal
        assert!(alanine.functional_groups.contains_key(&C_TERMINAL));
        alanine.functional_groups.insert(
            C_TERMINAL,
            GroupState::Modified(NamedMod::new(&POLYMER_DB, "Am").unwrap()),
        );
        snapshots.push(alanine.clone());

        // Add an amidation named modification to the N-terminal (ignoring that that's impossible)
        assert!(alanine.functional_groups.contains_key(&N_TERMINAL));
        alanine.functional_groups.insert(
            N_TERMINAL,
            GroupState::Modified(NamedMod::new(&POLYMER_DB, "Am").unwrap()),
        );
        snapshots.push(alanine.clone());

        // Out of functional groups, so adding more amidations changes nothing
        alanine.functional_groups.insert(
            N_TERMINAL,
            GroupState::Modified(NamedMod::new(&POLYMER_DB, "Am").unwrap()),
        );
        alanine.functional_groups.insert(
            C_TERMINAL,
            GroupState::Modified(NamedMod::new(&POLYMER_DB, "Am").unwrap()),
        );
        snapshots.push(alanine.clone());

        // But they can be replaced with bonds
        let peptide_bond = Bond::new(
            &POLYMER_DB,
            "Peptide",
            BondTarget {
                residue: 0,
                group: C_TERMINAL,
            },
        )
        .unwrap();
        alanine
            .functional_groups
            .insert(N_TERMINAL, GroupState::Donor(peptide_bond));
        snapshots.push(alanine.clone());

        // Residues can be protonated
        let proton =
            Modification::new(2, OffsetMod::new(&ATOMIC_DB, OffsetKind::Add, "p").unwrap());
        alanine.offset_modifications.push(proton);
        snapshots.push(alanine.clone());

        // Or can form other adducts
        let ca = Modification::new(
            1,
            OffsetMod::new(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap(),
        );
        alanine.offset_modifications.push(ca);
        snapshots.push(alanine.clone());

        // Removing the two protons...
        alanine.offset_modifications.remove(1);
        snapshots.push(alanine.clone());

        snapshots
    });

    #[test]
    fn monoisotopic_mass() {
        assert_eq!(
            RESIDUE_SERIES
                .iter()
                .map(Massive::monoisotopic_mass)
                .collect_vec(),
            vec![
                dec!(89.04767846918),
                dec!(71.03711378515),
                dec!(70.05309820224),
                dec!(69.06908261933),
                dec!(69.06908261933),
                dec!(52.04253351821),
                dec!(54.057086451452),
                dec!(94.018580154633870),
                dec!(92.004027221391870),
            ]
        );
    }

    #[test]
    fn average_mass() {
        assert_eq!(
            RESIDUE_SERIES
                .iter()
                .map(Massive::average_mass)
                .collect_vec(),
            vec![
                dec!(89.09330602867854225),
                dec!(71.07801959624870965),
                dec!(70.09325863743200710),
                dec!(69.10849767861530455),
                dec!(69.10849767861530455),
                dec!(52.07797220500217450),
                dec!(54.09252513824417450),
                dec!(94.16945048944377450),
                dec!(92.15489755620177450),
            ]
        );
    }

    #[test]
    fn charge() {
        assert_eq!(
            RESIDUE_SERIES.iter().map(Charged::charge).collect_vec(),
            vec![0, 0, 0, 0, 0, 0, 2, 4, 2]
        );
    }

    #[test]
    fn monoisotopic_mz() {
        assert_eq!(
            RESIDUE_SERIES.iter().map(Mz::monoisotopic_mz).collect_vec(),
            vec![
                None,
                None,
                None,
                None,
                None,
                None,
                Some(dec!(27.028543225726)),
                Some(dec!(23.50464503865846750)),
                Some(dec!(46.002013610695935))
            ]
        );
    }

    #[test]
    fn average_mz() {
        assert_eq!(
            RESIDUE_SERIES.iter().map(Mz::average_mz).collect_vec(),
            vec![
                None,
                None,
                None,
                None,
                None,
                None,
                Some(dec!(27.04626256912208725)),
                Some(dec!(23.5423626223609436250)),
                Some(dec!(46.07744877810088725))
            ]
        );
    }
}
