use crate::alloc::borrow::Cow;
use crate::alloc::vec::Vec;
use std::{cmp, iter, slice};

use goblin::pe;

use crate::{
    Machine, Object, ObjectSection, ObjectSegment, Relocation, SectionIndex, SectionKind, Symbol,
    SymbolIndex, SymbolKind, SymbolMap,
};

/// A PE object file.
#[derive(Debug)]
pub struct PeFile<'data> {
    pe: pe::PE<'data>,
    data: &'data [u8],
}

/// An iterator over the loadable sections of a `PeFile`.
#[derive(Debug)]
pub struct PeSegmentIterator<'data, 'file>
where
    'data: 'file,
{
    file: &'file PeFile<'data>,
    iter: slice::Iter<'file, pe::section_table::SectionTable>,
}

/// A loadable section of a `PeFile`.
#[derive(Debug)]
pub struct PeSegment<'data, 'file>
where
    'data: 'file,
{
    file: &'file PeFile<'data>,
    section: &'file pe::section_table::SectionTable,
}

/// An iterator over the sections of a `PeFile`.
#[derive(Debug)]
pub struct PeSectionIterator<'data, 'file>
where
    'data: 'file,
{
    file: &'file PeFile<'data>,
    iter: iter::Enumerate<slice::Iter<'file, pe::section_table::SectionTable>>,
}

/// A section of a `PeFile`.
#[derive(Debug)]
pub struct PeSection<'data, 'file>
where
    'data: 'file,
{
    file: &'file PeFile<'data>,
    index: SectionIndex,
    section: &'file pe::section_table::SectionTable,
}

/// An iterator over the symbols of a `PeFile`.
#[derive(Debug)]
pub struct PeSymbolIterator<'data, 'file>
where
    'data: 'file,
{
    index: usize,
    exports: slice::Iter<'file, pe::export::Export<'data>>,
    imports: slice::Iter<'file, pe::import::Import<'data>>,
}

/// An iterator over the relocations in an `PeSection`.
#[derive(Debug)]
pub struct PeRelocationIterator;

impl<'data> PeFile<'data> {
    /// Get the PE headers of the file.
    // TODO: this is temporary to allow access to features this crate doesn't provide yet
    #[inline]
    pub fn pe(&self) -> &pe::PE<'data> {
        &self.pe
    }

    /// Parse the raw PE file data.
    pub fn parse(data: &'data [u8]) -> Result<Self, &'static str> {
        let pe = pe::PE::parse(data).map_err(|_| "Could not parse PE header")?;
        Ok(PeFile { pe, data })
    }

    /// True for 64-bit files.
    #[inline]
    pub fn is_64(&self) -> bool {
        self.pe.is_64
    }

    fn section_alignment(&self) -> u64 {
        u64::from(
            self.pe
                .header
                .optional_header
                .map(|h| h.windows_fields.section_alignment)
                .unwrap_or(0x1000),
        )
    }
}

impl<'data, 'file> Object<'data, 'file> for PeFile<'data>
where
    'data: 'file,
{
    type Segment = PeSegment<'data, 'file>;
    type SegmentIterator = PeSegmentIterator<'data, 'file>;
    type Section = PeSection<'data, 'file>;
    type SectionIterator = PeSectionIterator<'data, 'file>;
    type SymbolIterator = PeSymbolIterator<'data, 'file>;

    fn machine(&self) -> Machine {
        match self.pe.header.coff_header.machine {
            // TODO: Arm/Arm64
            pe::header::COFF_MACHINE_X86 => Machine::X86,
            pe::header::COFF_MACHINE_X86_64 => Machine::X86_64,
            _ => Machine::Other,
        }
    }

    fn segments(&'file self) -> PeSegmentIterator<'data, 'file> {
        PeSegmentIterator {
            file: self,
            iter: self.pe.sections.iter(),
        }
    }

    fn section_by_name(&'file self, section_name: &str) -> Option<PeSection<'data, 'file>> {
        self.sections()
            .find(|section| section.name() == Some(section_name))
    }

    fn section_by_index(&'file self, index: SectionIndex) -> Option<PeSection<'data, 'file>> {
        self.sections().find(|section| section.index() == index)
    }

    fn sections(&'file self) -> PeSectionIterator<'data, 'file> {
        PeSectionIterator {
            file: self,
            iter: self.pe.sections.iter().enumerate(),
        }
    }

    fn symbol_by_index(&self, _index: SymbolIndex) -> Option<Symbol<'data>> {
        // TODO: return COFF symbols for object files
        None
    }

    fn symbols(&'file self) -> PeSymbolIterator<'data, 'file> {
        // TODO: return COFF symbols for object files
        PeSymbolIterator {
            index: 0,
            exports: [].iter(),
            imports: [].iter(),
        }
    }

    fn dynamic_symbols(&'file self) -> PeSymbolIterator<'data, 'file> {
        PeSymbolIterator {
            index: 0,
            exports: self.pe.exports.iter(),
            imports: self.pe.imports.iter(),
        }
    }

    fn symbol_map(&self) -> SymbolMap<'data> {
        // TODO: untested
        let mut symbols: Vec<_> = self
            .symbols()
            .map(|(_, s)| s)
            .filter(SymbolMap::filter)
            .collect();
        symbols.sort_by_key(|x| x.address);
        SymbolMap { symbols }
    }

    #[inline]
    fn is_little_endian(&self) -> bool {
        // TODO: always little endian?  The COFF header has some bits in the
        // characteristics flags, but these are obsolete.
        true
    }

    fn has_debug_symbols(&self) -> bool {
        // TODO: check if CodeView-in-PE still works
        for section in &self.pe.sections {
            if let Ok(name) = section.name() {
                if name == ".debug_info" {
                    return true;
                }
            }
        }
        false
    }

    fn entry(&self) -> u64 {
        self.pe.entry as u64
    }
}

impl<'data, 'file> Iterator for PeSegmentIterator<'data, 'file> {
    type Item = PeSegment<'data, 'file>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|section| PeSegment {
            file: self.file,
            section,
        })
    }
}

impl<'data, 'file> ObjectSegment<'data> for PeSegment<'data, 'file> {
    #[inline]
    fn address(&self) -> u64 {
        u64::from(self.section.virtual_address)
    }

    #[inline]
    fn size(&self) -> u64 {
        u64::from(self.section.virtual_size)
    }

    #[inline]
    fn align(&self) -> u64 {
        self.file.section_alignment()
    }

    fn data(&self) -> &'data [u8] {
        let offset = self.section.pointer_to_raw_data as usize;
        let size = cmp::min(self.section.virtual_size, self.section.size_of_raw_data) as usize;
        &self.file.data[offset..][..size]
    }

    fn data_range(&self, address: u64, size: u64) -> Option<&'data [u8]> {
        crate::data_range(self.data(), self.address(), address, size)
    }

    #[inline]
    fn name(&self) -> Option<&str> {
        self.section.name().ok()
    }
}

impl<'data, 'file> Iterator for PeSectionIterator<'data, 'file> {
    type Item = PeSection<'data, 'file>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(index, section)| PeSection {
            file: self.file,
            index: SectionIndex(index),
            section,
        })
    }
}

impl<'data, 'file> PeSection<'data, 'file> {
    fn raw_data(&self) -> &'data [u8] {
        let offset = self.section.pointer_to_raw_data as usize;
        let size = cmp::min(self.section.virtual_size, self.section.size_of_raw_data) as usize;
        &self.file.data[offset..][..size]
    }
}

impl<'data, 'file> ObjectSection<'data> for PeSection<'data, 'file> {
    type RelocationIterator = PeRelocationIterator;

    #[inline]
    fn index(&self) -> SectionIndex {
        self.index
    }

    #[inline]
    fn address(&self) -> u64 {
        u64::from(self.section.virtual_address)
    }

    #[inline]
    fn size(&self) -> u64 {
        u64::from(self.section.virtual_size)
    }

    #[inline]
    fn align(&self) -> u64 {
        self.file.section_alignment()
    }

    fn data(&self) -> Cow<'data, [u8]> {
        Cow::from(self.raw_data())
    }

    fn data_range(&self, address: u64, size: u64) -> Option<&'data [u8]> {
        crate::data_range(self.raw_data(), self.address(), address, size)
    }

    #[inline]
    fn uncompressed_data(&self) -> Cow<'data, [u8]> {
        // TODO: does PE support compression?
        self.data()
    }

    fn name(&self) -> Option<&str> {
        self.section.name().ok()
    }

    #[inline]
    fn segment_name(&self) -> Option<&str> {
        None
    }

    #[inline]
    fn kind(&self) -> SectionKind {
        if self.section.characteristics
            & (pe::section_table::IMAGE_SCN_CNT_CODE | pe::section_table::IMAGE_SCN_MEM_EXECUTE)
            != 0
        {
            SectionKind::Text
        } else if self.section.characteristics & pe::section_table::IMAGE_SCN_CNT_INITIALIZED_DATA
            != 0
        {
            SectionKind::Data
        } else if self.section.characteristics & pe::section_table::IMAGE_SCN_CNT_UNINITIALIZED_DATA
            != 0
        {
            SectionKind::UninitializedData
        } else {
            SectionKind::Unknown
        }
    }

    fn relocations(&self) -> PeRelocationIterator {
        PeRelocationIterator
    }
}

impl<'data, 'file> Iterator for PeSymbolIterator<'data, 'file> {
    type Item = (SymbolIndex, Symbol<'data>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(export) = self.exports.next() {
            let index = SymbolIndex(self.index);
            self.index += 1;
            return Some((
                index,
                Symbol {
                    kind: SymbolKind::Unknown,
                    // TODO: can we find a section?
                    section_index: None,
                    undefined: false,
                    global: true,
                    name: export.name,
                    address: export.rva as u64,
                    size: 0,
                },
            ));
        }
        if let Some(import) = self.imports.next() {
            let index = SymbolIndex(self.index);
            self.index += 1;
            let name = match import.name {
                Cow::Borrowed(name) => Some(name),
                _ => None,
            };
            return Some((
                index,
                Symbol {
                    kind: SymbolKind::Unknown,
                    section_index: None,
                    undefined: true,
                    global: true,
                    name,
                    address: 0,
                    size: 0,
                },
            ));
        }
        None
    }
}

impl Iterator for PeRelocationIterator {
    type Item = (u64, Relocation);

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}
