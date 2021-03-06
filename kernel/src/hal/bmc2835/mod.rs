mod uart;
pub use self::uart::*;
mod aux;
pub use self::aux::{MiniUart};
mod pl011;
pub use self::pl011::{Pl011,Pl011Interrupt,Pl011Flag,Pl011Error,Pl011FillLevel};
mod gpio;
pub use self::gpio::{Gpio,GpioPinFunctions,GpioPull,GpioEvent,gpio_config};
mod system_timer;
pub use self::system_timer::SystemTimer;
    
mod arm_timer;
pub use self::arm_timer::{ArmTimer,ArmTimerResolution};
mod interrupts;
pub use self::interrupts::{Interrupt,BasicInterrupt,GeneralInterrupt,NUM_INTERRUPTS,FIRST_BASIC_INTERRUPT};
mod irq_controller;
pub use self::irq_controller::IrqController;
mod led;
pub use self::led::{Led,LedType};

mod mailbox;
mod propertytags;

pub trait Bmc2835 {

    /// Basisadresse der Hardwaregeräte.
    fn device_base() -> usize {
        // Für den RPi 1 reicht eine Konstante.
        // Später sollte hier eine Fallunterscheidung entweder zur Compile-Zeit
        // oder zur Laufzeit gemacht werden.
        0x20000000
    }

    /// Gerätetypischer Offset der I/O-Adressen.
    fn base_offset() -> usize;

    /// Anfang der I/O-Adressen des Gerätes.
    fn base() -> usize {
        Self::device_base() + Self::base_offset()
    }

    /// Gibt den statischen Zeiger auf den I/O-Adressbereichs des Gerätes.
    fn get() -> &'static mut Self
        where Self: Sized {
        unsafe {
            &mut *(Self::base() as * mut Self)
        }
    }

}


pub use self::propertytags::{Tag,PropertyTagBuffer,BUFFER_SIZE};
pub use self::mailbox::{mailbox, Channel};

/// Art der verlangten Information
pub enum BoardReport {
    /// Version der Firmware
    FirmwareVersion,
    /// Code für den Computertyp (sollte 0 = Raspberry Pi sein)
    BoardModel,
    /// Version des Raspberrys
    BoardRevision,
    /// Seriennummer
    SerialNumber
}

/// Gibt Informationen über die Hardware
pub fn report_board_info(kind: BoardReport) -> u32 {  
    let mut prob_tag_buf: PropertyTagBuffer = PropertyTagBuffer::new();
    prob_tag_buf.init();
    let tag = match kind {
        BoardReport::FirmwareVersion => Tag::GetFirmwareVersion,
        BoardReport::BoardModel      => Tag::GetBoardModel,
        BoardReport::BoardRevision   => Tag::GetBoardRevision,
        BoardReport::SerialNumber    => Tag::GetBoardSerial
    };
    prob_tag_buf.add_tag_with_param(tag,None);
    let mb = mailbox(0);
    mb.write(Channel::ATags, prob_tag_buf.data_addr() as u32);
    mb.read(Channel::ATags);
    match prob_tag_buf.get_answer(tag) {
        Some(n) => n[0],
        _       => 0
    }
}

/// 
#[allow(dead_code)]
pub enum MemReport {
    /// Beginn des ARM-Speicherbereiches
    ArmStart,
    /// Größe des ARM-Speicherbereiches
    ArmSize,
    /// Beginn des Speicherbereiches des Videoprozessors
    VcStart,
    /// Größe des Speicherbereiches des Videoprozessors
    VcSize,
}

/// Gibt Informationen über die Speicheraufteilung
pub fn report_memory(kind: MemReport) -> usize {
    let mut prob_tag_buf = PropertyTagBuffer::new();
    prob_tag_buf.init();
    let tag = match kind {
        MemReport::ArmStart | MemReport::ArmSize => Tag::GetArmMemory,
        MemReport::VcStart  | MemReport::VcSize  => Tag::GetVcMemory
    };    
    prob_tag_buf.add_tag_with_param(tag,None);
    let mb = mailbox(0);
    mb.write(Channel::ATags, prob_tag_buf.data_addr() as u32);
    mb.read(Channel::ATags);
    let array = prob_tag_buf.get_answer(tag);
    match array {
        Some(a) => match kind {
            MemReport::ArmStart | MemReport::VcStart => a[0] as usize,
            MemReport::ArmSize  | MemReport::VcSize  => a[1] as usize
        },
        None => 0
    }
}
