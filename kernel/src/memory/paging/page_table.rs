/*
use core::ops::{Index, IndexMut};

use super::{PageTableEntry,PageTableEntryType,Pte};
use super::{LogicalAddress,PhysicalAddress};

#[repr(C)]
#[repr(align(1024))]
pub struct PageTable {
    table: [PageTableEntry;256]
}

impl PageTable {
    pub fn new() ->  PageTable {
        PageTable {
            table: [0;256]
        }
    }
    
    pub fn invalidate(&mut self) {
        for ndx in 0..256 {
            self.table[ndx] = Pte::new_entry(PageTableEntryType::Fault).entry();
        }
    }

    pub fn map(&mut self, paddr: PhysicalAddress, laddr: LogicalAddress) {
        
    }

    
}

impl Index<usize> for PageTable {
    type Output = PageTableEntry;

    fn index(&self, index: usize) -> &PageTableEntry {
        &self.table[index]
    }
}

impl IndexMut<usize> for PageTable {
    fn index_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.table[index]
    }
}
*/