# CLH-RUST

Нативная Rust-версия **Chunk Limiter HUD** для клиента [Horizon](https://github.com/horizon-rust/horizon). Мод считает настроенные блоки и сущности в загруженной клиентом области, показывает лимиты в HUD и добавляет красный outline перегруженным сущностям.

ID мода — `clh_rust`. Проект закреплён на ревизии Horizon `6cc93d13ced86a355d41764644506ae2550123cb` и использует native trait-mod API.

## Возможности

- область подсчёта от `1×1` до `32×32` чанков;
- точные лимиты для ID блоков и сущностей;
- встроенные группы `group:entities`, `group:mobs`, `group:items` и `group:pistons`;
- собственные группы блоков и сущностей с общим лимитом;
- HUD `текущее / лимит / осталось` с зелёным, жёлтым и красным статусом;
- красный outline сущностей при превышении лимита;
- фоновое сканирование блоков, не блокирующее игровой кадр;
- JSON-конфигурация в `config/clh-rust.json`;
- `O` — включить или выключить HUD;
- `L` — перечитать конфигурацию с диска.

Мод читает только чанки и сущности, которые уже известны клиенту, и не требует установки на сервер.

## Ограничения первой нативной версии

Текущий Horizon API в закреплённой ревизии не перечисляет на клиенте транспорт, снаряды, стойки для брони и локального игрока в bulk-снимке сущностей. Поэтому `group:minecarts` пока оставлен в конфигурации для совместимости, но вагонетки могут не попадать в счётчик. Произвольная подсветка всех перегруженных блоков и отдельный экран настроек также ещё не перенесены; параметры редактируются в JSON и применяются клавишей `L`.

## Конфигурация

После первого запуска создаётся `config/clh-rust.json`. Пример собственной группы:

```json
{
  "groups": {
    "ферма": {
      "type": "entity",
      "limit": 30,
      "members": [
        "minecraft:cow",
        "minecraft:sheep",
        "minecraft:pig"
      ]
    }
  }
}
```

Остальные поля и стандартные лимиты автоматически появятся в созданном файле.

## Проверка

Из WSL (первый запуск скачает закреплённую ревизию Horizon; общий target-кэш ускоряет повторные проверки):

```bash
cd /mnt/d/майн/CLH-RUST/mixins/clh_rust/api
CARGO_TARGET_DIR=/mnt/d/майн/horizon/target cargo +stable test
CARGO_TARGET_DIR=/mnt/d/майн/horizon/target cargo +stable clippy --all-targets -- -D warnings
```

## Сборка и запуск в Horizon

Нативный мод собирает launcher, потому что его crate линкуется прямо в клиент:

```bash
horizon-launcher native-build /mnt/d/майн/CLH-RUST --instance <instance> --trust-native-code
```

После сборки запускайте выбранный instance обычной командой Horizon Launcher.

## Упаковка `.hnmod`

Поместите этот репозиторий в `native/clh-rust` свежего checkout `horizon-mods`, затем из корня `horizon-mods` выполните:

```bash
python3 native/tools/native.py package clh-rust
```

Готовый файл создаётся в `native/target/clh_rust-0.1.0.hnmod`.

Полную сборку trait API выполняет Horizon Launcher командой `native-build`: отдельный `native.py check` пока не размещает launcher-only пакет `[api]` в проверочном дереве.

## Лицензия

CC0-1.0, как и исходная Java-версия.
