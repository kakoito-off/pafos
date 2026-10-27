# micro-os

Учебная микро-ОС на Rust для x86_64, запускается в QEMU.

Базируется на коде из цикла статей [Writing an OS in Rust](https://os.phil-opp.com)
(Philipp Oppermann), взята как отправная точка состояние после поста
"Heap Allocation" (freestanding binary → VGA text mode → testing →
CPU exceptions → double faults → hardware interrupts → paging → heap
allocation). Далее разрабатывается самостоятельно: шелл с набором
опциональных "микропрограмм" (калькулятор и т.п.), подключаемых в
сборку через Cargo features.

## Требования

- Rust nightly с GNU-toolchain (`nightly-x86_64-pc-windows-gnu` на Windows)
  и компонентами `rust-src`, `llvm-tools-preview`
- MinGW-w64 GCC в PATH (нужен хост-линкер для сборочных зависимостей)
- [`bootimage`](https://github.com/rust-osdev/bootimage) (`cargo install bootimage`)
- [QEMU](https://www.qemu.org/)

## Сборка и запуск

```powershell
.\build.ps1   # только собрать образ
.\run.ps1     # собрать и запустить в QEMU
```

Оба скрипта сами выставляют PATH, так что работают из любого открытого
терминала без ручной настройки окружения.

## Тестирование

```powershell
cargo test
```

Юнит-тесты и интеграционные тесты (`tests/`) гоняются прямо в QEMU
(headless, `-display none`), результат читается через serial-порт и
exit-код QEMU (`isa-debug-exit`).

## Структура

```
src/
  main.rs         точка входа (entry_point!), инициализация, демонстрация heap
  lib.rs          библиотечный крейт: общий API, test runner
  vga_buffer.rs   вывод текста в VGA text buffer (0xb8000)
  serial.rs       вывод в serial-порт (для тестов/отладки)
  gdt.rs          Global Descriptor Table + TSS (запасной стек для double fault)
  interrupts.rs   IDT, обработчики исключений, PIC, таймер, клавиатура
  memory.rs       пэйджинг: доступ к таблицам страниц, frame allocator
  allocator.rs    куча (global allocator) поверх пэйджинга
tests/
  basic_boot.rs       базовая проверка загрузки
  should_panic.rs     проверка, что паника действительно происходит
  stack_overflow.rs   проверка double-fault handler-а на переполнении стека
```

## Лицензия

Код унаследован от [phil-opp/blog_os](https://github.com/phil-opp/blog_os)
(MIT / Apache-2.0, см. `LICENSE-MIT` / `LICENSE-APACHE`) и дорабатывается
в рамках учебного проекта.
