use rust_decimal::Decimal;

use crate::{Bond, BondTarget, Charge, Charged, Massive, PolychemError, Result};

use super::polymer_database::{BondDescription, PolymerDatabase};

impl<'a, 'p> Bond<'a, 'p> {
    pub fn new(
        db: &'p PolymerDatabase<'a>,
        kind: impl AsRef<str>,
        acceptor: BondTarget<'p>,
    ) -> Result<Self> {
        let (kind, BondDescription { lost, .. }) = Self::lookup_description(db, kind)?;
        Ok(Self {
            kind,
            lost,
            acceptor,
        })
    }

    pub(crate) fn lookup_description(
        db: &'p PolymerDatabase<'a>,
        kind: impl AsRef<str>,
    ) -> Result<(&'p String, &'p BondDescription<'a>)> {
        let kind = kind.as_ref();
        db.bonds
            .get_key_value(kind)
            .ok_or_else(|| PolychemError::bond_lookup(kind).into())
    }
}

impl Massive for Bond<'_, '_> {
    fn monoisotopic_mass(&self) -> Decimal {
        -self.lost.monoisotopic_mass()
    }

    fn average_mass(&self) -> Decimal {
        -self.lost.average_mass()
    }
}

impl Charged for Bond<'_, '_> {
    fn charge(&self) -> Charge {
        -self.lost.charge()
    }
}

#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;
    use rust_decimal_macros::dec;

    use crate::{testing_tools::assert_miette_snapshot, AtomicDatabase, FunctionalGroup, Mz};

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

    static EMPTY_GROUP: FunctionalGroup = FunctionalGroup::new("", "");

    static EMPTY_TARGET: Lazy<BondTarget> = Lazy::new(|| BondTarget {
        residue: 0,
        group: EMPTY_GROUP,
    });

    #[test]
    fn errors() {
        let disulfide = Bond::new(&POLYMER_DB, "Disulfide", *EMPTY_TARGET);
        assert_miette_snapshot!(disulfide);
        let ionic = Bond::new(&POLYMER_DB, "Ionic", *EMPTY_TARGET);
        assert_miette_snapshot!(ionic);
    }

    #[test]
    fn monoisotopic_mass() {
        let glycosidic = Bond::new(&POLYMER_DB, "Glycosidic", *EMPTY_TARGET).unwrap();
        assert_eq!(glycosidic.monoisotopic_mass(), dec!(-18.01056468403));
        let stem = Bond::new(&POLYMER_DB, "Stem", *EMPTY_TARGET).unwrap();
        assert_eq!(stem.monoisotopic_mass(), dec!(-18.01056468403));
        let charged = Bond::new(&POLYMER_DB, "Charged", *EMPTY_TARGET).unwrap();
        assert_eq!(charged.monoisotopic_mass(), dec!(-2.014552933242));
    }

    #[test]
    fn average_mass() {
        let glycosidic = Bond::new(&POLYMER_DB, "Glycosidic", *EMPTY_TARGET).unwrap();
        assert_eq!(glycosidic.average_mass(), dec!(-18.01528643242983260));
        let stem = Bond::new(&POLYMER_DB, "Stem", *EMPTY_TARGET).unwrap();
        assert_eq!(stem.average_mass(), dec!(-18.01528643242983260));
        let charged = Bond::new(&POLYMER_DB, "Charged", *EMPTY_TARGET).unwrap();
        assert_eq!(charged.average_mass(), dec!(-2.014552933242));
    }

    #[test]
    fn charge() {
        let glycosidic = Bond::new(&POLYMER_DB, "Glycosidic", *EMPTY_TARGET).unwrap();
        assert_eq!(glycosidic.charge(), 0);
        let stem = Bond::new(&POLYMER_DB, "Stem", *EMPTY_TARGET).unwrap();
        assert_eq!(stem.charge(), 0);
        let charged = Bond::new(&POLYMER_DB, "Charged", *EMPTY_TARGET).unwrap();
        assert_eq!(charged.charge(), -2);
    }

    #[test]
    fn monoisotopic_mz() {
        let glycosidic = Bond::new(&POLYMER_DB, "Glycosidic", *EMPTY_TARGET).unwrap();
        assert_eq!(glycosidic.monoisotopic_mz(), None);
        let stem = Bond::new(&POLYMER_DB, "Stem", *EMPTY_TARGET).unwrap();
        assert_eq!(stem.monoisotopic_mz(), None);
        let charged = Bond::new(&POLYMER_DB, "Charged", *EMPTY_TARGET).unwrap();
        assert_eq!(charged.monoisotopic_mz(), Some(dec!(-1.007276466621)));
    }

    #[test]
    fn average_mz() {
        let glycosidic = Bond::new(&POLYMER_DB, "Glycosidic", *EMPTY_TARGET).unwrap();
        assert_eq!(glycosidic.average_mz(), None);
        let stem = Bond::new(&POLYMER_DB, "Stem", *EMPTY_TARGET).unwrap();
        assert_eq!(stem.average_mz(), None);
        let charged = Bond::new(&POLYMER_DB, "Charged", *EMPTY_TARGET).unwrap();
        assert_eq!(charged.average_mz(), Some(dec!(-1.007276466621)));
    }
}
