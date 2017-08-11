#![no_std]
#![no_main]
#![feature(
    alloc,                  // Nutzung der Alloc-Crate
    allocator_api,          // Nutzung der Allocator-API
    //allocator_internals,   // ???
    //abi_unadjusted,         //
    attr_literals,          // Literale in Attributen (nicht nur Strings)
    asm,                    // Assembler in Funktionen...
    associated_type_defaults, // Verknüpfung von Traits mit Typen
    // concat_idents,
    //collections,            // Nutzung des Collection-Crate
    const_fn,               // const Funktionen (für Constructoren)
    compiler_builtins_lib,  // Nutzung der Compiler-Buildins-Bibliothek (div, mul, ...)
    core_intrinsics,        // Nutzung der Intrinsics der Core-Bibliothek
    global_allocator,       // eigener globaler Allocator
    i128_type,              // 128-Bit-Typen
    inclusive_range_syntax, // Inklusiver Bereich mit "..."   
    iterator_step_by,       // Spezifische Schrittweite bei Iterationen
    lang_items,             // Funktionen interne Funktionen ersetzen (panic)
    linkage,                // Angaben zum Linktyp (z.B. Sichtbarkeit)
    naked_functions,        // Funktionen ohne Prolog/Epilog
    nonzero,                // Werte ohne Null (hier: usize)
    plugin,                 // Nutzung von Compiler-Plugins
    repr_align,             // Alignment
    // use_extern_macros,
    unique,                 // Unique-Pointer
    used,                   // Verbot, scheinbar toten Code zu eliminieren
)
]
#![plugin(compiler_error)]

/// Benutzte Crates
#[macro_use]
extern crate alloc;
#[macro_use]
//extern crate lazy_static;
extern crate bit_field;
//#[macro_use] extern crate collections;
extern crate compiler_builtins;

#[macro_use] mod aux_macros;
#[macro_use] mod debug;
mod panic;

mod sync;
use alloc::boxed::Box;

#[macro_use] mod hal;
use hal::board::{MemReport,BoardReport,report_board_info,report_memory};
use hal::entry::syscall;
use hal::cpu::{Cpu,ProcessorMode,MMU};
use core::mem::size_of;
mod mem;
use mem::PhysicalAddress;
use mem::paging::{Frame,FrameMethods};
use mem::paging::{PageDirectory,PageDirectoryEntry,PdEntry,PageDirectoryEntryType,MemoryAccessRight,MemType,PageTable,DomainAccess,PAGES_PER_SECTION};
use mem::paging::pde::Deb;
use mem::heap::BoundaryTagAllocator;

import_linker_symbol!(__text_end);
import_linker_symbol!(__data_start);
import_linker_symbol!(__data_end);
import_linker_symbol!(__bss_start);
import_linker_symbol!(__kernel_stack);

const IRQ_STACK_SIZE: usize = 2048;
pub  const INIT_HEAP_SIZE: usize = 25 * 4096; // 25 Seiten = 100 kB

#[global_allocator]
static mut HEAP: BoundaryTagAllocator = BoundaryTagAllocator::empty();
static PAGE_DIR: PageDirectory = PageDirectory::new();

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[no_mangle]      // Name wird für den Export nicht verändert
#[naked]          // Kein Prolog, da dieser den Stack nutzen würde, den wir noch nicht gesetzt haben
#[allow(unreachable_code)]
pub extern fn kernel_start() {
    // Zum Start existiert noch kein Stack. Daher setzen wir einen temporären Stack, der nach dem Textsegment liegt.
    // Das Symbol ist in "layout.ld" definiert.
    Cpu::set_stack(__kernel_stack as PhysicalAddress);
    // Nun kann die Größe des Speichers und damit die Adresse für den "echten" Stacks bestimmt werden
    Cpu::set_stack(determine_svc_stack());
    kernel_init();
    unreachable!();
}

#[inline(never)] // Verbietet dem Optimizer, kernel_init() und darin aufgerufene Funktionen mit kernel_start()
                 // zu verschmelzen. Dies würde wegen #[naked]/keinen Stack schief gehen
#[allow(unreachable_code)]
fn kernel_init() -> ! {
    report();
    init_mem();
    test();
    loop {}
    unreachable!();
}

fn determine_irq_stack() -> PhysicalAddress {
    let addr = (report_memory(MemReport::ArmSize) - 3) & 0xFFFFFFFC;
    addr
}

#[inline(never)]
fn determine_svc_stack() -> PhysicalAddress {
    let addr = ((report_memory(MemReport::ArmSize) - 3) & 0xFFFFFFFC) - IRQ_STACK_SIZE;
    addr
}

fn init_mem() {
    kprint!("Init stacks...");
    init_stacks();
    kprint!("done.\nInit heap...");
    init_heap();
    kprint!("done.\nInit pagetable...");
    init_paging();
    kprint!("done.\n");
}

fn init_stacks() {
    // Stack für die anderen Ausnahme-Modi.  Irq, Fiq, Abort und Undef teilen sich einen Stack, der System-Mode nutzt
    // den User-Mode-Stack und muss nicht gesetzt werden.
    let adr = determine_irq_stack();
    Cpu::set_mode(ProcessorMode::Irq);
    Cpu::set_stack(adr);
    Cpu::set_mode(ProcessorMode::Fiq);
    Cpu::set_stack(adr);
    Cpu::set_mode(ProcessorMode::Abort);
    Cpu::set_stack(adr);
    Cpu::set_mode(ProcessorMode::Undef);
    Cpu::set_stack(adr);
    // ...und zurück in den Svc-Mode
    Cpu::set_mode(ProcessorMode::Svc);
}

fn init_heap() {
    unsafe{
        HEAP.init(__bss_start as usize, INIT_HEAP_SIZE);
    }
}

///! Eine Coarse Table kann 1 MB (in 256 Blöcken a 4 kB) mappen.
///! Der komplette Kernel sollte kleiner als 1 MB sein, daher brauchen wir
///! für ihn lediglich eine Tabelle im Directory.
fn init_paging() {
    MMU::set_page_dir(&PAGE_DIR as *const _ as usize);
    // Standard ist Seitenfehler
    for section in 0..4096 {
        PAGE_DIR.set(section,PageDirectoryEntry::new(PageDirectoryEntryType::Fault));
    }
    kprint!("marked whole memory as fault.\n");
    // Der Kernel-Bereich wird auf sich selbst gemappt
    // Code 
    for section in Frame::from_addr(0).section()...Frame::from_addr(__text_end as usize).section() {
        let pde = PageDirectoryEntry::new(PageDirectoryEntryType::Section)
            .base_addr(Frame::from_nr(section * PAGES_PER_SECTION).start)
            .rights(MemoryAccessRight::SysRwUsrNone)
            .mem_type(MemType::NormalUncashed)
            .entry();
        let dpde = Deb::ug(pde);
        kprint!("identity mapping of section {} with base addr {} ({}):\n{:?}\n",section,
                Frame::from_nr(section * PAGES_PER_SECTION).start,
                pde,
                dpde);
        PAGE_DIR.set(section,pde);
    }
    /*
    // Kernel-Daten 
    for section in Frame::from_addr(__data_start as usize).section()...Frame::from_addr(__data_end as usize).section() {
        PAGE_DIR.set(section,
                     PageDirectoryEntry::new(PageDirectoryEntryType::Section)
                     .base_addr(Frame::from_nr(section * PAGES_PER_SECTION).start)
                     .rights(MemoryAccessRight::SysRwUsrNone)
                     .mem_type(MemType::NormalWT)
                     .never_execute(true)
                     .entry()
        );
        kprint!("identity mapping of secion {}.\n",section);
    }
    // Kernel-Heap
    for section in Frame::from_addr(__bss_start as usize).section()...Frame::from_addr(__bss_start as usize + INIT_HEAP_SIZE).section() {
        PAGE_DIR.set(section,
                     PageDirectoryEntry::new(PageDirectoryEntryType::Section)
                     .base_addr(Frame::from_nr(section * PAGES_PER_SECTION).start)
                     .rights(MemoryAccessRight::SysRwUsrNone)
                     .mem_type(MemType::NormalWT)
                     .never_execute(true)
                     .entry()
        );
        kprint!("identity mapping of secion {}.\n",section);
    }*/
    // Stacks
    for section in Frame::from_addr(determine_irq_stack() - 65556).section()..4096 {
        let pde = PageDirectoryEntry::new(PageDirectoryEntryType::Section)
                     .base_addr(Frame::from_nr(section * PAGES_PER_SECTION).start)
                     .rights(MemoryAccessRight::SysRwUsrNone)
                     .mem_type(MemType::NormalUncashed)
                     //.never_execute(true)
                     .entry();
        /*let dpde = Deb::ug(pde);
        kprint!("identity mapping of section {} with base addr {} ({}):\n{:?}\n",section,
                Frame::from_nr(section * PAGES_PER_SECTION).start,
                pde,
                dpde);*/
        PAGE_DIR.set(section,pde);
    }
    // Den Stack und alles drüber (eigentlich nur die HW) brauchen wir auch:
    /*
    for page in 447..4096 {
        let pde: PageDirectoryEntry;
        pde = PdEntry::new(PdEntryType::Section).base_addr(page << 20).rights(MemoryAccessRight::SysRwUsrNone).mem_type(MemType::NormalUncashed).entry();  // Identitätsmapping
        PAGE_DIR[page as usize] = pde;
    }
     */
    //let kernel_pt = Box::new(PageTable::new());
    //for page in 0..256 {
    //    kernel_pt[0] = Pte::new_entry(PageTableEntryType::SmallCodePage).base_addr(page)
    //}
    //let kernel_pt_addr = Box::into_raw(kernel_pt);
    //let pde = PdEntry::new(PdEntryType::CoarsePageTable).base_addr((kernel_pt_addr as u32) << 20).entry();
    //let kernel_pt = unsafe{ Box::from_raw(kernel_pt_addr)};
    
    MMU::set_domain_access(0,DomainAccess::Manager);
    MMU::start();
    kprint!("MMU aktiviert.\n");
}
 
fn report() {
    kprint!("aihPOS"; RED);
    kprint!(" Version {}\n",VERSION; RED);
    let  (firmware_version, board_model, board_revision,serial) = (report_board_info(BoardReport::FirmwareVersion),
                                                                   report_board_info(BoardReport::BoardModel),
                                                                   report_board_info(BoardReport::BoardRevision),
                                                                   report_board_info(BoardReport::SerialNumber));
    if board_model == 0 {
        kprint!("Raspberry Pi");
    } else {
        kprint!("Unbekanntes Board");
    }
    let (serial_high,serial_low) = (serial >> 16, serial & 0xFFFF);
    kprint!(", Version {:#0x}, Seriennummer {:04x}.{:04x}\n",board_revision,serial_high,serial_low);
    kprint!("Firmwareversion {:0x}\n",firmware_version);
    kprint!("Speicherlayout:\n");
    kprint!("0x{:08x} ({:10}): Anfang Kernelcode\n",kernel_start as usize,kernel_start as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Ende Kernelcode\n",__text_end as usize,__text_end as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Anfang Kerneldaten\n",__data_start as usize, __data_start as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Anfang Pagedirectory\n",&PAGE_DIR as *const _ as usize, &PAGE_DIR as *const _ as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Ende Pagedirectory\n",&PAGE_DIR as *const _ as usize + size_of::<PageDirectory>(), &PAGE_DIR as *const _ as usize + size_of::<PageDirectory>(); WHITE);
    kprint!("0x{:08x} ({:10}): Ende Kerneldaten\n",__data_end as usize,__data_end as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Anfang Kernelheap\n",__bss_start as usize, __bss_start as usize; WHITE);
    kprint!("0x{:08x} ({:10}): Initiales Ende Kernelheap\n",__bss_start as usize + INIT_HEAP_SIZE, __bss_start as usize + INIT_HEAP_SIZE; WHITE);
    kprint!("0x{:08x} ({:10}): TOS System\n",determine_svc_stack() as usize, determine_svc_stack() as usize; WHITE);
    kprint!("0x{:08x} ({:10}): TOS Interrupt\n",determine_irq_stack() as usize, determine_irq_stack() as usize; WHITE);
}

fn test() {
    kprint!("Calling system.\n");
    let ret=syscall!(23,1,2);
    kprint!("Returned from system call: {}.\n",ret);
    /*
    let mut frame_manager = FrameManager::new();
    frame_manager.mark_not_available(0..0x0002ffff);
    //kprint!("ff: {}\n",frame_manager.first_free);
    for _ in 0..1 {
        let adr: u32 = frame_manager.allocate();
        kprint!("Neuer Frame @ {:08x}\n",adr);
    }
    frame_manager.release(0x00090000);
    for _ in 0..2 {
        let adr: u32 = frame_manager.allocate();
        kprint!("Neuer Frame @ {:08x}\n",adr);
    }
     */
    {
        let v1 = Box::new(0);
        let v2 = Box::new((23,42));
        let v3 = Box::new(1); 
        kprint!("v1 = {}, v2 = {:?}, v3 = {}.\n",*v1,*v2,*v3);
        drop(v1);
    }
    // Das folgende sollte eine Schutzverletzung geben
    unsafe{
        let pt: *mut u32 = 0x1000000 as *mut u32;
        *pt = 42;
    }
    kprint!("Ich lebe noch.");
    debug::blink(debug::BS_HI);
}

#[inline(never)]
#[no_mangle]
#[allow(private_no_mangle_fns,unused_variables)]
#[linkage="weak"] // Verhindert, dass der Optimierer die Funktion eliminiert
pub fn svc_service_routine(nr: u32, arg1: u32, arg2: u32)  -> u32
{
    kprint!("System Call #{:X} with parameter {} and {}\n",nr,arg1,arg2);
    42
}
