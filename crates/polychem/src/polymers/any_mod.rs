use rust_decimal::Decimal;

use crate::{
    atoms::atomic_database::AtomicDatabase, AnyMod, AnyModification, Charge, Charged, Massive,
    Modification, NamedMod, OffsetKind, OffsetMod, Result,
};

use super::polymer_database::PolymerDatabase;

impl<'a, 'p> AnyMod<'a, 'p> {
    pub fn named(db: &'p PolymerDatabase<'a>, abbr: impl AsRef<str>) -> Result<Self> {
        Ok(Self::Named(NamedMod::new(db, abbr)?))
    }

    pub fn offset(
        db: &'a AtomicDatabase,
        kind: OffsetKind,
        formula: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::Offset(OffsetMod::new(db, kind, formula)?))
    }
}

impl<'a, 'p> From<NamedMod<'a, 'p>> for AnyMod<'a, 'p> {
    fn from(value: NamedMod<'a, 'p>) -> Self {
        Self::Named(value)
    }
}

impl<'a, 'p> From<OffsetMod<'a>> for AnyMod<'a, 'p> {
    fn from(value: OffsetMod<'a>) -> Self {
        Self::Offset(value)
    }
}

impl<'a, 'p, K: Into<AnyMod<'a, 'p>>> From<K> for AnyModification<'a, 'p> {
    fn from(value: K) -> Self {
        Modification::new(1, value.into())
    }
}

// NOTE: There are crates for automating more of this code generation, but odds are that I'll only need to do this sort
// of enum dispatch for AnyMod — it doesn't seem worth a dependency and cluttering lib.rs with attributes
macro_rules! dispatch {
    ($self:expr, $method:ident) => {
        match $self {
            AnyMod::Named(m) => m.$method(),
            AnyMod::Offset(m) => m.$method(),
        }
    };
}

impl Massive for AnyMod<'_, '_> {
    fn monoisotopic_mass(&self) -> Decimal {
        dispatch!(self, monoisotopic_mass)
    }

    fn average_mass(&self) -> Decimal {
        dispatch!(self, average_mass)
    }
}

impl Charged for AnyMod<'_, '_> {
    fn charge(&self) -> Charge {
        dispatch!(self, charge)
    }
}

#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;
    use rust_decimal_macros::dec;

    use crate::{testing_tools::assert_miette_snapshot, Mz};

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
        let magnesium = AnyMod::named(&POLYMER_DB, "Mg");
        assert_miette_snapshot!(magnesium);
        let potassium = AnyMod::named(&POLYMER_DB, "K");
        assert_miette_snapshot!(potassium);
        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H[2O]");
        assert_miette_snapshot!(water_gained);
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H[2O]");
        assert_miette_snapshot!(water_lost);
    }

    #[test]
    fn from_impls() {
        let named_mod = NamedMod::new(&POLYMER_DB, "Am").unwrap();
        let named_any_mod: AnyMod = named_mod.into();
        let named_any_modification: AnyModification = named_mod.into();
        let named_any_any_modification: AnyModification = named_any_mod.clone().into();
        assert_eq!(
            named_mod.monoisotopic_mass(),
            named_any_mod.monoisotopic_mass()
        );
        assert_eq!(
            named_mod.monoisotopic_mass(),
            named_any_modification.monoisotopic_mass()
        );
        assert_eq!(
            named_mod.monoisotopic_mass(),
            named_any_any_modification.monoisotopic_mass()
        );

        let offset_mod = OffsetMod::new(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        let offset_any_mod: AnyMod = offset_mod.clone().into();
        let offset_any_modification: AnyModification = offset_mod.clone().into();
        let offset_any_any_modification: AnyModification = offset_any_mod.clone().into();
        assert_eq!(
            offset_mod.monoisotopic_mass(),
            offset_any_mod.monoisotopic_mass()
        );
        assert_eq!(
            offset_mod.monoisotopic_mass(),
            offset_any_modification.monoisotopic_mass()
        );
        assert_eq!(
            offset_mod.monoisotopic_mass(),
            offset_any_any_modification.monoisotopic_mass()
        );
    }

    #[test]
    fn monoisotopic_mass() {
        // Masses checked against https://www.unimod.org/modifications_list.php
        let amidation = AnyMod::named(&POLYMER_DB, "Am").unwrap();
        assert_eq!(amidation.monoisotopic_mass(), dec!(-0.98401558291));
        let acetylation = AnyMod::named(&POLYMER_DB, "Ac").unwrap();
        assert_eq!(acetylation.monoisotopic_mass(), dec!(42.01056468403));
        let deacetylation = AnyMod::named(&POLYMER_DB, "DeAc").unwrap();
        assert_eq!(deacetylation.monoisotopic_mass(), dec!(-42.01056468403));
        let calcium = AnyMod::named(&POLYMER_DB, "Ca").unwrap();
        assert_eq!(calcium.monoisotopic_mass(), dec!(38.954217236560870));

        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        assert_eq!(water_gained.monoisotopic_mass(), dec!(18.01056468403));
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap();
        assert_eq!(water_lost.monoisotopic_mass(), dec!(-18.01056468403));
        // Masses checked against https://bioportal.bioontology.org/ontologies/UBERON
        let ca_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap();
        assert_eq!(ca_gained.monoisotopic_mass(), dec!(39.961493703181870));
        let ca_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "Ca-2e").unwrap();
        assert_eq!(ca_lost.monoisotopic_mass(), dec!(-39.961493703181870));
    }

    #[test]
    fn average_mass() {
        // Masses checked against https://www.unimod.org/modifications_list.php
        let amidation = AnyMod::named(&POLYMER_DB, "Am").unwrap();
        assert_eq!(amidation.average_mass(), dec!(-0.98476095881670255));
        let acetylation = AnyMod::named(&POLYMER_DB, "Ac").unwrap();
        assert_eq!(acetylation.average_mass(), dec!(42.03675822590033060));
        let deacetylation = AnyMod::named(&POLYMER_DB, "DeAc").unwrap();
        assert_eq!(deacetylation.average_mass(), dec!(-42.03675822590033060));
        let calcium = AnyMod::named(&POLYMER_DB, "Ca").unwrap();
        assert_eq!(calcium.average_mass(), dec!(39.069648884578600));

        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        assert_eq!(water_gained.average_mass(), dec!(18.01528643242983260));
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap();
        assert_eq!(water_lost.average_mass(), dec!(-18.01528643242983260));
        // Masses checked against https://bioportal.bioontology.org/ontologies/UBERON
        let ca_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap();
        assert_eq!(ca_gained.average_mass(), dec!(40.076925351199600));
        let ca_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "Ca-2e").unwrap();
        assert_eq!(ca_lost.average_mass(), dec!(-40.076925351199600));
    }

    #[test]
    fn charge() {
        let amidation = AnyMod::named(&POLYMER_DB, "Am").unwrap();
        assert_eq!(amidation.charge(), 0);
        let acetylation = AnyMod::named(&POLYMER_DB, "Ac").unwrap();
        assert_eq!(acetylation.charge(), 0);
        let deacetylation = AnyMod::named(&POLYMER_DB, "DeAc").unwrap();
        assert_eq!(deacetylation.charge(), 0);
        let calcium = AnyMod::named(&POLYMER_DB, "Ca").unwrap();
        assert_eq!(calcium.charge(), 1);

        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        assert_eq!(water_gained.charge(), 0);
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap();
        assert_eq!(water_lost.charge(), 0);
        let ca_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap();
        assert_eq!(ca_gained.charge(), 2);
        let ca_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "Ca-2e").unwrap();
        assert_eq!(ca_lost.charge(), -2);
    }

    #[test]
    fn monoisotopic_mz() {
        let amidation = AnyMod::named(&POLYMER_DB, "Am").unwrap();
        assert_eq!(amidation.monoisotopic_mz(), None);
        let acetylation = AnyMod::named(&POLYMER_DB, "Ac").unwrap();
        assert_eq!(acetylation.monoisotopic_mz(), None);
        let deacetylation = AnyMod::named(&POLYMER_DB, "DeAc").unwrap();
        assert_eq!(deacetylation.monoisotopic_mz(), None);
        let calcium = AnyMod::named(&POLYMER_DB, "Ca").unwrap();
        assert_eq!(calcium.monoisotopic_mz(), Some(dec!(38.954217236560870)));

        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        assert_eq!(water_gained.monoisotopic_mz(), None);
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap();
        assert_eq!(water_lost.monoisotopic_mz(), None);
        let ca_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap();
        assert_eq!(ca_gained.monoisotopic_mz(), Some(dec!(19.980746851590935)));
        let ca_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "Ca-2e").unwrap();
        assert_eq!(ca_lost.monoisotopic_mz(), Some(dec!(-19.980746851590935)));
    }

    #[test]
    fn average_mz() {
        let amidation = AnyMod::named(&POLYMER_DB, "Am").unwrap();
        assert_eq!(amidation.average_mz(), None);
        let acetylation = AnyMod::named(&POLYMER_DB, "Ac").unwrap();
        assert_eq!(acetylation.average_mz(), None);
        let deacetylation = AnyMod::named(&POLYMER_DB, "DeAc").unwrap();
        assert_eq!(deacetylation.average_mz(), None);
        let calcium = AnyMod::named(&POLYMER_DB, "Ca").unwrap();
        assert_eq!(calcium.average_mz(), Some(dec!(39.069648884578600)));

        let water_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "H2O").unwrap();
        assert_eq!(water_gained.average_mz(), None);
        let water_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "H2O").unwrap();
        assert_eq!(water_lost.average_mz(), None);
        let ca_gained = AnyMod::offset(&ATOMIC_DB, OffsetKind::Add, "Ca-2e").unwrap();
        assert_eq!(ca_gained.average_mz(), Some(dec!(20.0384626755998)));
        let ca_lost = AnyMod::offset(&ATOMIC_DB, OffsetKind::Remove, "Ca-2e").unwrap();
        assert_eq!(ca_lost.average_mz(), Some(dec!(-20.0384626755998)));
    }
}
