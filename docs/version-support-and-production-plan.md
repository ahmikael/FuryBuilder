# FuryBuilder: поддержка всех версий и production-ready план

> Статус: план принят; реализация ведётся по рабочему чеклисту в §21. Version foundation, project data, component facade, Java по major/download/manual, client JAR download, JAR-инспекция, baseline-каталог, manual assets, explicit placeholder, exact-version server provisioning, EULA consent, общий download pipeline, Tauri components API + TS-клиент, mock HTTP-тесты, hardening manual/placeholder/relink, lifecycle store, Welcome screen, Project gate, Teardown при закрытии, Recoverable autosave, lifecycle-тесты, каркас wizard, Basics, Version, Source Empty/Flat, Source Import, Source Vanilla и Components закрыты; идёт Review.
> Дата: 2026-11-22; обновлено 2026-11-24 (project schema v2 only, snapshot v3 only, checkpoint metadata v2, component manager, client JAR catalog, manual assets, explicit placeholder, server exact version, EULA per JAR/version, shared download pipeline, components commands/events, mock HTTP suite, manual/placeholder/relink tests, project lifecycle, welcome screen, project gate, session teardown, recoverable autosave, lifecycle tests, project wizard shell, basics with project folder and mode requirements, version catalog and selection step, version-aware flat source, folder/archive import with safe staging and import strategy, vanilla source with version guard, bounded generation estimate and disk preflight, components step with auto/manual/existing choices, explicit placeholder and catalog counts).
> Цель: превратить FuryBuilder из редактора одной версии в полноценный редактор миров и структур Minecraft Java Edition с понятным первым запуском, автоматической подготовкой компонентов и чистым интерфейсом.

---

## 1. Коротко: что должно получиться

1. **Версия Minecraft становится главным свойством проекта.** При создании проекта пользователь выбирает версию, а FuryBuilder сам подбирает Java, клиентские ассеты, серверный JAR и форматы мира.
2. **Скачивание — основной путь, ручной выбор — запасной.** Java, текстуры/клиентский JAR и сервер скачиваются из официальных источников по кнопке, с проверкой хэша. Ручной путь остаётся для offline, своего `.minecraft`, своего `java.exe` и своего `server.jar`.
3. **Настройки больше не дублируют создание проекта.** В `Settings` остаются внешний вид, установленные компоненты, обновления, диагностика и backend. Выбор «скачать или указать путь» живёт в мастере создания проекта и в настройках уже открытого проекта.
4. **Сцена не создаётся, пока нет проекта.** Никакого демо-мира, пустого рендерера и «серого» viewport до выбора/создания проекта. Вместо этого — экран приветствия и список недавних проектов.
5. **Интерфейс становится иерархичным.** Сверху — только уровень приложения, слева — World/Project, справа — инспектор и инструменты, снизу — блоки/слои/анализ/активность, плюс четыре режима World/Build/Paint/Analyze. Случайные бейджи и 16 перекрывающихся вкладок уходят.
6. **Сцена получает нормальное окружение.** Небо, солнце/луна, освещение Minecraft, день/ночь, погода, аккуратная сетка, границы чанков и улучшенная навигация.
7. **Поддержка версий расширяется поэтапно и честно.** Сначала современные версии с полным редактированием, затем старые семейства, затем legacy через конвертацию. Bedrock и модифицированные миры не выдаются за поддержанные.
8. **Каталог как в игре.** В каждой версии доступны все её ванильные блоки и предметы — с состояниями, категориями, иконками и честными пометками для technical/creative-only; новые блоки версии появляются автоматически.

---

## 2. Что есть сейчас (факты, от которых отталкиваемся)

| Область | Текущее состояние | Файлы |
| --- | --- | --- |
| Версия мира | `VersionProfile` в проекте; baseline 1.21.4 / DataVersion 4189 write-verified; глобальные `TARGET_*` удалены | `crates/minecraft-version/`, `crates/project-store/src/manifest.rs` |
| Импорт | Только classic Anvil 1.21.4; остальные версии определяются и отклоняются | `crates/world-io/src/anvil.rs`, `world_import.rs` |
| Экспорт мира | Backend умеет экспортировать полную папку, но one-click UI нет | `crates/world-io/src/world_export.rs` |
| Структуры | Только Java `.nbt`; `.schem`, `.schematic`, `.litematic` не включены | `crates/world-io/src/structure.rs`, `java_structure.rs` |
| Java | Менеджер скачивает только Temurin 21 | `crates/runtime-manager/src/types.rs` |
| Сервер | Provisioning принимает точную версию/DataVersion/Java major, кэш по версии; desktop-настройки и генерация пока baseline | `crates/runtime-manager/src/server/` |
| Текстуры | Managed client JAR скачивается по version JSON; локальный клиент по-прежнему обнаруживается | `crates/component-manager/src/client_jar.rs`, `crates/asset-source/src/lib.rs` |
| Настройки | Содержат отдельные блоки Java, сервера и ассетов; мастер Vanilla World дублирует выбор Java/сервера | `SettingsDialog.tsx`, `JavaRuntimeSection.tsx`, `MinecraftServerSection.tsx`, `VanillaWorldWizard.tsx` |
| Проект | `project.json` schema 2: `target` из `VersionProfile` + `components`; версия больше не глобальная | `crates/project-store/src/manifest.rs` |
| Snapshot проекта | Формат `.fwl` v3: `profileId`, `dataVersion`, layout, biome mode + world/dimensions | `crates/project-store/src/snapshot.rs` |
| Чекпоинты | Metadata v2 + полный snapshot: profileId, label, reason, editorState; до 5 | `crates/project-store/src/store.rs` |
| Автосейв | Каждые 30 секунд, только для crash recovery | `ProjectManager.tsx`, `store.rs` |
| Undo | До 100 транзакций в памяти | `crates/world-application/src/service.rs` |
| Нижняя панель | 16 вкладок в одном горизонтальном списке | `features/workspace/BottomPanel.tsx`, `editor/layout.ts` |
| Верхняя панель | Кроме меню и оконных кнопок показывает бейдж `Active palette block` | `features/workspace/Toolbar.tsx` |
| Viewport | Всегда создаёт renderer и загружает demo/core snapshot даже без проекта | `features/viewport/Viewport.tsx` |
| Рендер | `MeshBasicMaterial`, тёмный фон, нет освещения; `GridHelper` 256×256 на y=64.04 | `renderer/viewport-renderer.ts` |
| Границы чанков | Жёстко `yBottom = -64`, `yTop = 72` | `renderer/viewport-renderer.ts` |
| Дерево мира | Три захардкоженных измерения, Overworld всегда “loaded” | `features/workspace/WorldTree.tsx` |
| Блоки и предметы | Курируемый каталог ~60 блоков остаётся fallback/quick picks и для валидации; `asset-source::catalog` строит полный каталог точной версии из client JAR (blockstates, item definitions/models, textures, lang, tags, atlases) | `crates/asset-source/src/catalog.rs`, `crates/world-domain/src/registry.rs` |

Вывод: проблема не в одном экране. Нужно ввести **версию как сквозную модель**, затем аккуратно провести её через backend, компоненты, проект, UI и рендер.

---

## 3. Продуктовые принципы

1. **Один проект — одна версия.** Версия хранится в проекте, а не в глобальных настройках.
2. **Компоненты принадлежат проекту, но кэшируются глобально.** Java, client JAR и server JAR можно переиспользовать между проектами одной версии.
3. **Auto-first, manual-second.** Везде сначала предлагается автоматическая загрузка с проверкой, затем ручной путь, затем явный placeholder/read-only режим.
4. **Никаких скрытых скачиваний и запусков.** «Авто» означает «рекомендуемый вариант по умолчанию», а не «скачать без ведома пользователя». EULA и запуск сервера всегда требуют явного согласия.
5. **Сцена — только для открытого проекта.** Никаких фальшивых demo-миров в рабочем окне.
6. **Не писать в формат, который не проверен.** Если версия read-only, редактор честно показывает read-only и предлагает конвертацию/паспорт, а не делает вид, что умеет сохранять.
7. **Оригинал неприкосновенен.** Импорт, конвертация версии и экспорт работают через копию/overlay; исходный мир не изменяется.
8. **Один смысл — одно место.** Статус ядра, состояние сохранения, выбор инструмента, настройка компонентов не дублируются в трёх местах.
9. **Прогрессивное раскрытие.** Базовый сценарий — 3–5 понятных шагов; продвинутые вещи (snapshots, Build DSL, MCP activity, performance) спрятаны, но доступны.
10. **Честная матрица поддержки.** Каждая версия и каждый формат имеют статус: full / read-only / convert / passthrough / out of scope.
11. **Контент версии — единственный источник истины.** Палитра, валидация состояний и экспорт используют один и тот же каталог блоков/предметов проекта; новые блоки версии не требуют ручного обновления списка.
12. **Режимы и контекст вместо хрома.** Пользователь выбирает задачу (World/Build/Paint/Analyze), а интерфейс показывает только её инструменты; один смысл живёт в одном месте.
13. **shadcn-first, default-first.** Любая UI-работа начинается с проверки готового shadcn-компонента через skill/MCP; компонент используется с дефолтным стилем shadcn, без перекраски и форка. Custom — только если готового нет, и в том же визуальном языке.

### 3.1. Политика чистой перезаписи

FuryBuilder ещё нигде не установлен и не содержит пользовательских проектов. Поэтому совместимость со старыми внутренними форматами не нужна:

- **Миграций нет.** `project.json` сразу v2, `.fwl` сразу v3, checkpoint metadata сразу v2. Старые схемы не читаем и не пишем.
- **Legacy-кода нет.** Не оставляем двойные reader/writer, deprecated-обёртки, feature flags «старый/новый путь», `_old`-модули, compatibility shims и мёртвый UI.
- **Удаляем сразу.** Старый код удаляется в том же изменении, где появляется замена; «почистим потом» запрещено.
- **Демо пересоздаём.** `examples/furybuilder-demo.furybuilder` генерируется под актуальную схему, а не читается через миграцию.
- **Тесты — только текущий контракт.** Тесты на старые схемы и удалённое поведение не пишем.
- **Fallback — только продуктовый.** Placeholder-текстуры, manual path, retry и error states остаются. Compatibility-fallback «прочитать старый формат» — нет.
- **Legacy Minecraft — это фича, а не мусор.** Поддержка и конвертация 1.8–1.12 и pre-1.8 остаются в объёме, но внутри используются только актуальные модели и один актуальный код пути.
- **Критерий чистоты:** в коде нет migration/deprecated/dual-path веток, старых схем, неиспользуемых экспортов и комментариев «оставлено для совместимости».

---

## 4. Область поддержки версий

### 4.1. Что считаем «всеми версиями»

- **Minecraft Java Edition, release-версии** — основной scope.
- **Snapshots** — опционально, в advanced-режиме; по умолчанию read-only/convert, без гарантии записи.
- **Bedrock Edition** — вне scope: другой формат (LevelDB), другие ассеты, другая модель мира.
- **Модифицированные профили и кастомные измерения** — определяются, но не получают full-edit; как минимум passthrough/read-only.
- **Legacy pre-1.13** — поддержка через отдельное семейство; сначала импорт/конвертация, затем, если оправдано, нативное редактирование.

### 4.2. Уровни поддержки

| Уровень | Что значит | Что можно делать |
| --- | --- | --- |
| **Full native** | FuryBuilder понимает формат и проверяет запись | Создать, импортировать, редактировать блоки/биомы, сохранить, экспортировать, генерировать |
| **Read + write subset** | Чтение и ограниченная запись проверены | Редактировать только поддерживаемые поля; остальное сохраняется byte-for-byte |
| **Read-only / passthrough** | Формат распознан, но запись не проверена | Анализ, просмотр, копирование без изменений, экспорт исходника |
| **Convert** | Версия не редактируется напрямую, но может быть приведена к поддерживаемой | Импорт → конвертация в целевую версию; оригинал сохраняется |
| **Out of scope** | Формат вне Java/Anvil-линейки | Явный отказ с объяснением |

### 4.3. Целевая матрица

| Семейство | Версии | Цель | Этап |
| --- | --- | --- | --- |
| Modern classic Anvil | 1.18–1.21.x | **Full native**: импорт, блоки, биомы, экспорт, генерация, ассеты | P1 |
| Modern split layout | 26.1–26.3 | **Full native после адаптера**: `dimensions/`, `players/`, split region | P1/P3 |
| Classic Anvil, flattened | 1.13–1.17 | **Full native**: другой chunk/biome layout, те же принципы | P2 |
| Classic Anvil, numeric IDs | 1.8–1.12.2 | **Native adapter**: числовые ID + metadata, биомы-колонки; сначала импорт/экспорт, затем редактирование | P3 |
| Legacy McRegion | pre-1.13 (1.0–1.12) | **Convert/passthrough**: `.mcr` читается и конвертируется; нативное редактирование — только если оправдано | P3 |
| Snapshots | выбранные | **Read/convert**, без обещания записи | P3 |
| Bedrock | все | **Out of scope** | — |

Важно: **не обещать «полную поддержку» до появления фикстур и проверки записи**. В UI статус версии должен быть виден до создания проекта.

### 4.4. Что будет с текущими ограничениями

| Ограничение | Что делаем |
| --- | --- |
| KL-05: только 1.21.4 | Закрываем поэтапно матрицей версий |
| KL-06: bare `.mca`/`region/` | Оставляем запрет, но объясняем, как выбрать полный мир/архив |
| KL-07: только block states и биомы | Остаётся для native; для остального — read-only/passthrough |
| KL-08/09: derived data и block entities | Не симулируем; явно показываем preserve/rebuild и ограничения |
| KL-10: экспорт мира без UI | Добавляем one-click экспорт полной папки |
| KL-11/12: только `.nbt` | Добавляем `.schem`/`.schematic`/`.litematic` по этапам |
| KL-13/14: только Java 21 / server 1.21.4 | Java и server подбираются под версию |
| KL-17–22: ограничения редактора | Часть улучшаем (выделение, кисти), часть остаётся осознанной |
| KL-28: только Overworld | Добавляем измерения проекта и потоковую загрузку по выбранному измерению |
| KL-29/30: нет ассетов, мало моделей | Автозагрузка client JAR, полный каталог блоков/предметов и расширение model pipeline |
| KL-33–37: MCP | Версия берётся из проекта; read-only политика сохраняется |
| KL-42: нет удаления/переноса проекта | Добавляем безопасные операции в Project Settings |

---

## 5. Архитектура: версия как сквозная модель

### 5.1. Новый crate `minecraft-version`

Добавить отдельный crate (или модуль в `world-io`), который не зависит от Tauri/UI:

```text
minecraft-version
  VersionId
  VersionProfile
  VersionFamily
  VersionCatalog
  JavaRequirement
  WorldCapabilities
  AssetProfile
  ContentCatalogProfile
  ServerProfile
  StructureProfile
```

`VersionProfile` — это не просто строка версии, а набор проверенных фактов:

- `minecraftVersion`, `dataVersion`, `releaseType`;
- `worldLayout`: classic Anvil / split 26.1+ / McRegion / unknown;
- `chunkFamily`: modern paletted / section biomes / numeric IDs / legacy;
- `biomeFormat`: 4×4×4 palette / column / section;
- `dimensionProfiles`: id, kind, minY, height, folder;
- `java`: minimum/recommended major;
- `resourcePackFormat`, `datapackFormat`;
- `contentCatalog`: block/item registry format, blockstate/item model format, creative categories, components support;
- `structureFormats`: `.nbt`, `.schem`, `.schematic`, `.litematic`;
- `server`: доступность official JAR, `level-type` syntax, generation capabilities;
- `capabilities`: read/edit/write/export/generate/assets;
- `notes`: честные ограничения.

Каталог версий:

1. Скачивает официальный `version_manifest_v2.json` и кэширует его с TTL/offline-fallback.
2. Показывает release-версии (и snapshots в advanced).
3. Подтягивает version JSON для Java major и download URLs.
4. После скачивания client/server JAR читает внутренний `version.json` и фиксирует exact `dataVersion`, `resourcePackFormat`, `datapackFormat`.
5. Собирает `VersionProfile` из семейства + метаданных, а не из разрозненных констант.
6. Строит `CatalogSnapshot` блоков и предметов из client JAR и official metadata; кэширует по `version + jarSha1 + catalogSchema`.

### 5.2. Один внутренний мир + набор codec-ов

Вместо «одного Anvil-ридера на 1.21.4»:

```text
WorldCodec
  inspect(root) -> WorldInspection
  load_overlay(root, profile) -> WorldOverlay
  decode_chunk(raw, profile) -> NormalizedChunk
  encode_chunk(base_nbt, chunk, profile) -> NbtFile
  write_region(...)
  export_world(...)
```

Реализации:

- `ClassicAnvilCodec` — 1.13–1.21.x, параметризованный chunk/biome family.
- `SplitAnvilCodec` — 26.1+ (`dimensions/`, `players/`).
- `LegacyMcrCodec` — pre-1.13.
- `PassthroughCodec` — read-only/unknown, byte-for-byte.
- `ConvertCodec` — обёртка над official server upgrade или внутренним маппингом.

Внутренняя модель (`world-domain`) остаётся нормализованной:

- block state = name + properties;
- dimensions — из `VersionProfile`, а не из трёх констант;
- biome — нормализованные 4×4×4 ячейки; для старых версий допускается column-mode с явным преобразованием при чтении/записи;
- minY/height — из профиля.

### 5.3. Project schema v2

`project.json` должен хранить:

```jsonc
{
  "projectSchemaVersion": 2,
  "target": {
    "minecraftVersion": "1.21.4",
    "dataVersion": 4189,
    "profileId": "java-classic-1.21.4",
    "javaMajor": 21,
    "resourcePackFormat": 46,
    "datapackFormat": 61
  },
  "components": {
    "java": { "mode": "managed", "id": "...", "sha256": "..." },
    "assets": {
      "mode": "managed",
      "clientJarSha1": "...",
      "catalogId": "java-1.21.4-<hash>",
      "catalogHash": "...",
      "packStack": [{ "path": "...", "sha1": "..." }]
    },
    "server": { "mode": "managed", "version": "1.21.4", "sha1": "..." }
  }
}
```

Правила:

- Абсолютные пути не являются источником истины. Проект хранит logical id + hash, а путь резолвится из глобального кэша; если компонент пропал — «Relink».
- `catalogId`/`catalogHash` фиксируют каталог блоков и предметов проекта. Если каталог пропал, он пересобирается из того же client JAR; молчаливая подмена на «последний» каталог запрещена.
- `projectSchemaVersion` — единственная актуальная схема (v2). v1 не читается, миграция и backup-путь не пишутся (§3.1).
- Snapshot `.fwl` получает v3: `profileId`, `dataVersion`, dimension layout, biome mode. v2 не читается.
- Checkpoint metadata получает v2: `profileId`, `editorState` (слои/сцена/параметрика), label, reason. v1 не читается.
- API `world_info`, project summary, MCP metadata возвращают фактическую версию проекта, а не глобальную 1.21.4.

### 5.4. Полный каталог блоков и предметов версии

Сейчас `BlockRegistry` — курируемый список ~60 блоков для 1.21.4; предметов нет. Цель — палитра «как creative inventory», но для редактирования мира: **все ванильные блоки и предметы точной версии проекта**, включая технические, creative-only и блоки без предметной формы.

#### Источники

- **client JAR:** `blockstates`, block/item models, item definitions (в новых версиях), textures, `lang`, tags, sounds.
- **official registry metadata:** registry names, numeric IDs для legacy, data components для 1.20.5+.
- **server JAR / generated reports:** если доступны — для сверки registry и item groups.
- **1.13+:** named block states и item IDs.
- **1.8–1.12:** numeric ID + metadata mapping.
- **pre-1.8:** fallback-каталог и конвертация.

Реализация `P0-24`: `asset-source::AssetCatalogService` читает выбранный client JAR и строит
`CatalogSnapshot`: все blockstate-файлы, item definitions/models, parent-цепочки моделей,
существующие textures, `en_us`, прямые tags и atlas sources. `catalogHash` считается по
содержимому каталога, версии, DataVersion и SHA-1 JAR; `catalogId` имеет вид
`java-<version>-<hash16>`. Кэш пока process-local. Точные in-code defaults, категории, флаги,
item components, транзитивные tags и persistent cache остаются в P1.

#### Модель каталога

`CatalogEntry`:

- `id`, `legacyId`/`metadata`, `displayName`, `category`, `tags`, `icon`/`texture`, `aliases`;
- `blockStates`: все property keys, допустимые значения, default state, варианты;
- `item`: item form, stack size, max damage, data components (read/preserve);
- флаги: `placeable`, `hasItem`, `obtainable`, `creativeOnly`, `technical`, `fluid`, `airlike`, `blockEntity`, `transparent`, `fullCube`, `gravity`, `waterloggable`, `tinted`, `hasVariants`;
- `introducedIn`/`removedIn`/`versionRange`.

`CatalogSnapshot` = entries + categories + counts + hash. `CatalogDiff` = добавленные/удалённые/переименованные записи, изменения состояний, тегов и категорий.

#### Версионность

- Snapshot кэшируется по `version + clientJarSha1 + packStackHash + catalogSchema`.
- Для full-версий каталог можно предсобирать в CI; для неизвестных/advanced — строить в runtime.
- Проект фиксирует `catalogId` + hash. Если каталог отсутствует — пересобрать из того же JAR, а не подменять «последним».
- Переименования и aliases: старый ID → новый ID; неизвестные импортированные состояния сохраняются и никогда не подменяются молча.
- Технические и creative-only блоки доступны для размещения, но явно помечены; fluids/airlike обрабатываются правилами редактора.
- Предметы: все предметы доступны в поиске; placeable-предметы дублируются в блок-палитре; item-only записи используются для контейнеров, item frames, armor stands; неподдерживаемые контексты — read-only.
- Модифицированные/кастомные registry: полный каталог не обещаем; unknown сохраняется, режим passthrough, явное предупреждение.
- Новые блоки и предметы версии не требуют ручного списка: каталог строится из client JAR, а `CatalogDiff` показывает, что появилось, исчезло или переименовано между версиями.

#### UI

- Категории как в creative inventory: Building Blocks, Colored, Natural, Functional, Redstone, Tools & Utilities, Combat, Food & Drinks, Ingredients, Spawn Eggs; плюс `All`, `Recent`, `Favorites`.
- Строительные блоки — первыми; предметы — отдельная вкладка/фильтр.
- Поиск по имени, ID и тегу; фильтры: placeable, has item, technical, color, shape, version.
- 3D-иконка/превью, state picker со всеми свойствами и default state.
- Бейдж `not in this version`, disabled-причина, предупреждение для unknown.
- Локализованные имена (en/ru + fallback), aliases для старых названий.
- Виртуализированный список, ленивый atlas, поисковый индекс; каталог на 10k+ записей не должен тормозить.

#### Лицензии и границы

- Ассеты берутся только из client JAR, скачанного пользователем; Mojang-файлы не бандлятся.
- Кэшировать можно метаданные и сгенерированные иконки, но не распространять их отдельно от приложения.
- Bedrock-каталог и полное редактирование item components/NBT — вне scope; предметы поддерживаются для выбора, размещения и preserve.


### 5.5. Capabilities вместо if-ов по всему UI

`VersionProfile.capabilities` управляет:

- доступными инструментами (biome paint, heightmap, generators);
- доступными форматами структур;
- доступными пресетами генерации;
- предупреждениями о derived data;
- read-only/full-edit статусом;
- текстами в UI и MCP.

Это убирает «серые» кнопки без объяснения: кнопка либо скрыта, либо disabled с причиной.

---

## 6. Компоненты: Java, текстуры, сервер

### 6.1. Один менеджер компонентов

Вместо трёх независимых потоков (`runtime-manager`, `asset-source`, `server`) — единый facade:

```text
components_status(project/version)
components_resolve(kind, version)
components_install(kind, version, expectedHash)
components_cancel(kind)
components_relink(kind, path)
components_remove(kind, id)
components_verify(kind, id)
```

Facade реализован в `crates/component-manager`; чистые типы статусов — в `crates/component-model`. `status` сверяет компоненты с target проекта, `resolve` возвращает проверенный id/path/hash, `relink` не копирует и не редактирует пользовательский файл, `remove` удаляет только FuryBuilder-owned данные, а `install` отказывается от неизвестной версии/хэша. Java и server download умеют cooperative cancel с очисткой partial-файлов; client JAR download добавлен в P0-23.

Tauri shell держит один экземпляр facade и отдаёт его через `components_status`, `components_resolve`, `components_inventory`, `components_java_release`, `components_assets_release`, `components_server_release`, `components_install`, `components_cancel`, `components_relink`, `components_remove`, `components_verify`. Установка стримит `furybuilder-component-progress`, успешная мутация — `furybuilder-component-changed`, ошибка — `furybuilder-component-error` с тем же стабильным кодом, что и rejected command. TS-клиент `apps/desktop/src/lib/components-api.ts` — единственная точка входа для UI.

Типы:

- `java` — managed Temurin или manual executable;
- `assets` — managed client JAR или manual `.minecraft` root/client JAR/resource pack/placeholder;
- `server` — managed official server JAR или manual server JAR.

Общее:

- progress events, cancel, retry;
- SHA-1/SHA-256 + size verification;
- atomic install в per-user cache;
- manifest с версией, URL, хэшем, размером, датой;
- disk-space preflight;
- offline/fallback;
- proxy из настроек;
- allowlist хостов: Mojang, Adoptium, GitHub release assets;
- никаких произвольных URL из version JSON.

Реализовано в `crates/download-pipeline`: один HTTPS-клиент с точным host allowlist на каждом redirect hop, proxy из явной настройки или стандартных переменных, offline-режим, bounded retry для transport/408/425/429/5xx, disk preflight с запасом, потоковая проверка размера и SHA-1/SHA-256, запись в same-directory temp и atomic replace. Java, client JAR, server JAR и version manifest используют один и тот же pipeline; checksum/size/policy ошибки не ретраятся.

### 6.2. Java

- Определять требуемую Java по version JSON (`javaVersion.majorVersion`).
- Скачивать **Temurin JRE нужной major-версии** (8, 16, 17, 21, 25 — по профилю), а не всегда 21.
- Кэш: `<app-data>/components/java/<major>/<release-id>`.
- Ручной путь: выбрать `java.exe`; проверить major, архитектуру, vendor, запуск `-version`.
- Если manual Java новее рекомендуемой — разрешить с предупреждением; если старее — блокировать.
- Проект хранит logical id + hash; при отсутствии — Relink/скачать.
- В настройках — только инвентарь и удаление/проверка, не отдельный «setup wizard».

### 6.3. Текстуры и клиентские ассеты

- Скачивать **client JAR** выбранной версии из официального version JSON.
- Проверять SHA-1 и размер.
- Кэш: `<app-data>/components/assets/<version>/client.jar` + manifest.
- Managed download реализован в `component-manager`: preview берётся из `version_manifest_v2` → version JSON, рядом с JAR сохраняется exact `client.json`, манифест фиксирует SHA-1/размер/DataVersion/pack formats; cached JAR перепроверяется и при повреждении скачивается заново.
- Внутри JAR читать:
  - `version.json`;
  - `assets/minecraft/blockstates`;
  - `assets/minecraft/models` и item definitions/models;
  - `assets/minecraft/textures`;
  - `assets/minecraft/lang` и tags.

- Инспекция JAR и baseline-каталог: `asset-source::catalog` (см. `docs/catalog.md`).
- Если текстуры отсутствуют в client JAR — добирать из asset index (по необходимости), но не скачивать весь asset index без нужды.
- Строить atlas и полный каталог блоков/предметов под версию (§5.4); каталог кэшировать по `version + jarSha1 + packStackHash + catalogSchema`.
- Resource packs — per-project ordered stack, с проверкой `pack_format` под версию.
- Ручной путь: `.minecraft` root, `versions/<id>`, `client.jar`, resource pack `.zip`/folder.
- Ручной `client.jar` принимается после проверки внутренних `version.json` и DataVersion; SHA-1 сверяется с соседним `<id>.json`/`client.json`, а при его отсутствии файл используется с явным предупреждением. Объявленный mismatch отклоняется.
- Placeholder — **явный** режим с предупреждением, а не молчаливый fallback.
- Кэш atlas-а: `version + clientJarSha1 + packStackHash + rendererSchema`.
- Mojang assets не распространяются вместе с приложением; только официальная загрузка пользователем.

### 6.4. Сервер

- Скачивать official `server.jar` точной версии из Mojang manifest.
- Проверять SHA-1, размер, внутренний `version.json`/entry point/Java major.
- Кэш: `<app-data>/components/servers/<version>/server.jar`.
- Ручной путь: выбрать `server.jar`, проверить те же факты.
- EULA привязывать к хэшу конкретного JAR и версии.
- Запуск — только после явного consent, в изолированной temp-папке, с bounded memory/time и kill-on-close.
- Для версий без официального server JAR — генерация недоступна, но empty/flat/import работают; UI объясняет почему.

Реализовано: `ServerTarget` несёт точные version/DataVersion/Java major, Mojang resolution идёт через общий `VersionCatalog`, манифест кэша хранит версию/URL/SHA-1/размер/дату, а `server.json` schema 3 помнит отдельный выбор и отдельное явное EULA-согласие для каждой пары «точная версия + SHA-1 JAR». Запуск требует одновременно записи EULA для конкретного JAR/версии и отдельного per-run consent; `.furybuilder-run.json` schema 2 фиксирует версию, SHA-1 JAR, время EULA и consent до создания процесса, а импорт завершённого мира сверяет эти факты с отчётом. Изолированная генерация остаётся baseline-возможностью до отдельного расширения.

### 6.5. Где что живёт

| Действие | Где |
| --- | --- |
| Выбрать «скачать Java / указать путь» | Мастер создания проекта, шаг Components |
| Выбрать «скачать текстуры / указать путь / placeholder» | Мастер создания проекта, шаг Components |
| Посмотреть/пересобрать каталог блоков и предметов | Project Settings → World (автоматически из client JAR) |
| Выбрать «скачать сервер / указать путь» | Только Vanilla World в мастере |
| Посмотреть установленные компоненты, размер, удалить, проверить | Settings → Components |
| Перепривязать компонент для открытого проекта | Project Settings → Components |
| Принять EULA | Мастер, финальный шаг Vanilla World |
| Сгенерировать мир | Мастер, Vanilla World |
| Управлять обновлениями приложения | Settings → Updates |
| Диагностика | Settings → Diagnostics |

Так исчезает дублирование: **настройки — это склад и обслуживание, мастер — это выбор и первичная установка, Project Settings — это ремонт конкретного проекта.**

---

## 7. Жизненный цикл проекта и мастер создания

### 7.1. Состояния приложения

```text
CLOSED (экран приветствия)
  └─ CREATE/OPEN → PREPARING (компоненты, импорт, генерация)
        └─ READY (редактор + сцена)
              ├─ SAVING
              ├─ ERROR (retry/repair)
              └─ CLOSED
```

Правила:

- `PREPARING` показывает progress и не монтирует viewport.
- `READY` — единственное состояние, где создаётся `ViewportRenderer`.
- При закрытии проекта renderer dispose, world store reset, autosave stop, jobs cancel.
- При ошибке подготовки проект не открывается «наполовину»; пользователь видит причину и Retry.

### 7.2. Экран приветствия

Вместо пустого viewport:

- бренд и короткое описание;
- **недавние проекты** (имя, версия, последний save, размер, badge recoverable);
- кнопки: `New project`, `Open project`, `Import world`;
- ссылка на `Open demo` (опционально, read-only, не автоматически);
- версия приложения и статус ядра — в status bar/настройках, без дублей.

### 7.3. Мастер создания проекта

Полноэкранный wizard, не dialog поверх сцены.

**Шаг 1. Basics**

- имя проекта;
- папка проекта (по умолчанию `Documents/FuryBuilder`);
- режим: Empty, Flat, Import World, Vanilla World;
- описание требований для каждого режима.

**Шаг 2. Version**

- поиск и группировка релизов (1.21, 1.20, …);
- latest stable / recommended;
- snapshot toggle в advanced;
- карточка версии: Java major, world height, layout, asset format, datapack format (если известен), support badge;
- содержимое версии: количество блоков и предметов, доступные категории, технические/creative-only блоки;
- предупреждения: read-only/convert/out of scope;
- для Import версия определяется автоматически, пользователь только подтверждает.

**Шаг 3. Source**

- Empty: пустой мир.
- Flat: параметры платформы/слоёв, высота, биом; версия учитывается.
- Import: выбор папки/архива, анализ (DataVersion, layout, dimensions, chunks, blockers), выбор `Import as-is` или `Convert to <target>`.
- Vanilla: seed, preset, radius, structures, datapacks; предупреждение о времени/диске; EULA.

**Шаг 4. Components**

- Java: `Auto (recommended)` / `Manual` / `Use existing`.
- Assets: `Auto download client JAR` / `Manual .minecraft/client.jar` / `Placeholder`; после выбора показывать количество блоков/предметов и версию каталога.
- Server: только для Vanilla: `Auto download` / `Manual` / `Use existing`.
- Каждая карточка показывает: версия, размер, источник, статус, checksum, кнопку `Download`/`Choose`.
- Ошибки: нет сети, хост недоступен, checksum mismatch, нет места, Java несовместима.

**Шаг 5. Review**

- сводка: имя, версия, режим, источник, Java, assets, server, ожидаемый размер, время;
- предупреждения: derived data, block entities, downgrade невозможен, EULA, read-only;
- кнопка `Create project`.

**Шаг 6. Preparing**

- шаги с progress: metadata → Java → client JAR → catalog/atlas → server → generate/import → verify → ready;
- Cancel/Retry;
- после успеха — редактор; после ошибки — остаёмся в wizard, скачанное не теряется.

### 7.4. Настройки открытого проекта

`Project Settings` (из меню Project и из бейджа проекта):

- **General**: имя, папка, creation mode (read-only), source, размер.
- **Version**: текущая версия, DataVersion, profile, support badge; `Upgrade version`.
- **Components**: Java/assets/server; relink, repair, re-download.
- **World**: dimensions, height, biome mode, source integrity, derived-data policy, content catalog (version, hash, counts).
- **Storage**: snapshots, checkpoints, cache size, open folder.
- **Danger**: delete project (в корзину), relocate folder, export project archive.

### 7.5. Смена версии проекта

Технически возможно не всё. Честная политика:

- **Same version / re-pin components** — безопасно, без конвертации.
- **Upgrade** (например, 1.20.6 → 1.21.4):
  - preflight: target profile, Java/assets/server, доступность конвертера;
  - обязательный checkpoint;
  - работа с копией проекта, оригинал не трогаем;
  - внутренний adapter для same-family;
  - official server upgrade (DataFixerUpper) для старых → новых;
  - preview отчёта: сколько блоков/биомов смаплено, какие unknown;
  - staged → verify → atomic rename;
  - при ошибке оригинал остаётся нетронутым.
- **Downgrade** — не поддерживаем: Minecraft не гарантирует обратную совместимость. Вместо этого:
  - экспорт копии в текущей версии;
  - создание нового проекта в старой версии;
  - импорт структур/участков, если формат позволяет.
- **Смена creation mode** — нельзя; только новый проект.

### 7.6. Открытие существующего проекта

- Если компоненты на месте — обычное открытие.
- Если Java/assets/server отсутствуют — `Needs components`: показать, что именно, и предложить Relink/Download.
- Если каталог блоков/предметов отсутствует или повреждён — пересобрать из client JAR; не открывать урезанную палитру без явного предупреждения.
- Если проект новее схемы — отказ с просьбой обновить приложение.
- Если версия проекта стала read-only после обновления — открыть read-only и предложить конвертацию.
- Если исходный мир/папка недоступны — предупредить, но не блокировать редактирование snapshot-а; экспорт исходника будет недоступен.

---

## 8. Импорт, экспорт, генерация и форматы

### 8.1. Импорт миров

- Определение версии по `level.dat` (DataVersion), layout по папкам, chunk family по profile.
- Для full native: source copy → overlay → snapshot.
- Для read-only: анализ + passthrough, без обещания редактирования.
- Для legacy: `.mcr` чтение; опционально конвертация в выбранную modern-версию.
- Для split 26.1+: чтение `dimensions/` и `players/`; отдельный codec.
- Для архива `.zip`: безопасная распаковка в temp, проверка traversal/symlink/размеров, затем обычный анализ.
- Всегда показывать: version, layout, dimensions, chunks, block entities/entities, blockers, limitations.
- Показывать количество блоков/предметов, unknown states и записи, которых нет в целевой версии.

### 8.2. Экспорт миров

- One-click `Export world` в Project Manager/Project Settings.
- Всегда полная папка мира, не отдельный `.mca`.
- Same-version export через соответствующий codec.
- Каждый записываемый block state валидируется по каталогу целевой версии; unknown/removed — warning или blocker, без молчаливой подмены.
- Untouched files/records — byte-for-byte.
- Dirty regions — rebuild + reopen + verify.
- Derived data policy:
  - `Preserve` — оставить heightmaps/light как есть;
  - `Vanilla rebuild` — удалить/инвалидировать, чтобы engine пересчитал.
- Destination must not exist; staged folder; atomic rename; source unchanged.
- Опционально `Export as .zip`.
- Для read-only версии — только passthrough copy исходника, с явным статусом.

### 8.3. Структуры и обмен

Целевые форматы:

| Формат | Импорт | Экспорт | Примечание |
| --- | --- | --- | --- |
| Java structure `.nbt` | Да | Да | Основной, версия/DV в файле |
| Sponge `.schem` v1–v3 | Да | Да | Второй адаптер, маппинг palette |
| Legacy `.schematic` | Да | Ограниченно | Только для legacy-совместимых блоков |
| Litematica `.litematic` | Да (read) | Нет/позже | Сложные metadata; не обещать запись |
| `.mcstructure` (Bedrock) | Нет | Нет | Out of scope |

- Импорт: выбор формата по расширению, проверка версии/палитры, preview, вставка в проект или создание нового.
- Экспорт: формат выбирается по версии проекта; предупреждения о потере entities/block entities/unknown tags.
- Round-trip verification: re-encode → re-parse → checksum.

### 8.4. Генерация мира

- Только official server, точная версия проекта.
- Java подбирается по версии; server JAR скачивается/выбирается в wizard.
- EULA — per JAR/version, explicit; consent record binds the exact SHA-1 and Minecraft version.
- Пресеты и `level-type` — version-specific mapping:
  - modern: `minecraft:normal` / `minecraft:flat` / `minecraft:large_biomes` / `minecraft:amplified`;
  - 1.13–1.15: `default` / `flat` / `largeBiomes` / `amplified`;
  - 1.8–1.12: `DEFAULT` / `FLAT` / `LARGEBIOMES` / `AMPLIFIED`.
- `server.properties`, `generate-structures`, datapack format, view distance, memory, threads — из profile + UI.
- Bounded area, time limit, cancel, log capture, progress.
- Generated world автоматически анализируется и импортируется; при несовпадении версии — blocker.
- Если версия не имеет официального server JAR — генерация disabled с объяснением.

### 8.5. Полный список форматов

- Мир: classic Anvil, split 26.1+, McRegion; `.zip` world archive.
- Проект: `.furybuilder/` folder, optional `.furybuilder.zip` export/import.
- Структуры: `.nbt`, `.schem`, `.schematic`, `.litematic` (read).
- Ассеты: client JAR, resource pack folder/zip.
- Компоненты: managed cache manifests.
- Никаких Bedrock-форматов.

---

## 9. Чистка интерфейса

### 9.1. Верхняя панель

Сейчас в `Toolbar.tsx` кроме меню и оконных кнопок есть бейдж `Active palette block`. Это дублирует палитру, Inspector и viewport HUD.

Новая верхняя панель:

- логотип + `FuryBuilder`;
- `Project` / `Edit` / `View` / `AI` (по мере необходимости);
- проект: имя + версия + dirty-индикатор;
- undo/redo, save;
- command palette, settings;
- window controls справа;
- drag region и double-click maximize сохраняются.

Убрать:

- бейдж активного блока;
- дублирующий core status (он остаётся только в `StatusBar`);
- дублирующий save status из performance-панели;
- лишние разделители и мелкие кнопки, которые есть в меню/палитре.

### 9.2. Схема рабочего пространства

```text
┌────────────────────────────────────────────────────────────┐
│ Top bar: brand | Project Edit View AI | project/version | … │
├──────────────┬──────────────────────────────┬──────────────┤
│ World tree   │ 3D viewport                  │ Inspector    │
│ + project    │                              │ / Toolbox    │
│   source     │                              │              │
├──────────────┴──────────────────────────────┴──────────────┤
│ Bottom: Blocks | Layers | Analysis | Activity              │
├────────────────────────────────────────────────────────────┤
│ Status bar: core | version | dimension | selection | coords│
└────────────────────────────────────────────────────────────┘
```

- Панели resizable, размеры сохраняются.
- Левая: World Tree + Project Source.
- Центр: viewport.
- Правая: `Inspector` + `Toolbox` (вкладки).
- Низ: 4 осмысленные вкладки.
- Верх: только уровень приложения.

### 9.3. Нижняя панель: аудит 16 вкладок

| Текущая вкладка | Что делает | Решение |
| --- | --- | --- |
| **Palette** | Полный каталог блоков/предметов версии: свойства, поиск, категории, replace source | Оставить → `Blocks` |
| **Tools** | Дублирует toolbar/viewport: tool, brush radius, fill/replace/delete | Удалить вкладку; инструменты — в viewport rail + `Toolbox` |
| **Terrain** | Terrain brush | Перенести в правый `Toolbox` |
| **Heightmap** | Heightmap preview/apply | Перенести в `Toolbox` |
| **Biomes** | Biome paint | Перенести в `Toolbox` |
| **Generators** | Roads/rivers/trees и т.п. | Перенести в `Toolbox` |
| **Layers** | Слои, маски, protected regions | Оставить → `Layers` |
| **Templates** | Библиотека шаблонов | Перенести в `Toolbox` / asset library |
| **Objects** | Семантические объекты сцены | Перенести в `Toolbox` или `Inspector → Scene` |
| **Parametric** | Параметрические рецепты | Перенести в `Toolbox` |
| **Build DSL** | JSON-рецепты, preview | Перенести в `Toolbox` (advanced) |
| **Observation** | Карты, скриншоты, material/biome/height | Оставить → `Analysis` sub-tab |
| **Validation** | Проверки и fixes | Оставить → `Analysis` sub-tab |
| **Console** | Core status, last operation, warnings | Перенести в `Activity` |
| **History** | Undo/redo и транзакции + MCP activity | Перенести в `Activity` |
| **Warnings** | Только warnings последней операции | Объединить с `Analysis` + inline-уведомления |

Итог снизу: **Blocks, Layers, Analysis, Activity**.

Почему:

- 16 вкладок не помещаются и превращаются в горизонтальную свалку.
- `Tools` дублирует кнопки в viewport.
- `Warnings` — это не отдельный раздел, а результат операции/валидации.
- `Console` и `History` — это активность, а не ежедневный инструмент.
- `Observation` и `Validation` — анализ, им место рядом.
- Все специализированные инструменты — в `Toolbox`, сгруппированные и version-aware.

### 9.4. Правая панель: Inspector + Toolbox

`Inspector` (как сейчас):

- выбранный блок;
- координаты, chunk, dimension;
- selection/box;
- clipboard;
- project summary;
- save state.

`Toolbox` (новое):

- категории: Terrain, Heightmap, Biomes, Generators, Templates, Objects, Parametric, Build DSL;
- только инструменты, доступные для версии проекта;
- поиск по инструментам;
- pinned/favorites;
- описание и требования (selection, assets, version);
- предупреждения о derived data.

Это убирает «инструменты в нижней панели» и даёт им нормальное место.

### 9.5. Настройки

Новая структура `Settings`:

1. **Appearance** — тема, UI scale, reduced motion.
2. **Components** — установленные Java/assets/servers, размер, путь, verify/remove, open folder; без мастеров создания.
3. **Updates** — как сейчас.
4. **Diagnostics** — логи, open folder, clear.
5. **Advanced** — backend info, proxy, renderer/debug, cache.

Убрать из Settings:

- `Java & World Generation` как отдельный setup;
- `DependencyReadiness`;
- дублирующий `MinecraftServerSection` с выбором/скачиванием;
- повторный `MinecraftAssetsSection` setup.

Оставить общие компоненты `ComponentCard`, `ComponentPicker`, `ComponentStatus`, которые используются и в мастере, и в Settings, но с разными действиями. Это не дублирование UI, а переиспользование.

### 9.6. Project Manager

- В режиме «нет проекта» — это часть Welcome, а не dialog поверх пустого viewport.
- В режиме «проект открыт» — dialog для переключения/создания.
- Вкладки: `Projects`, `New project`, `Import / Export`; `Checkpoints` — внутри Project Settings (или отдельная вкладка только для текущего проекта).
- Карточка проекта: имя, версия, режим, размер, last save, recoverable, support badge.
- Действия: Open, Rename, Checkpoints, Duplicate, Delete (Recycle Bin), Relocate, Export project.

### 9.7. Viewport HUD

Сейчас слишком много плавающих элементов: dimension/coords, tool rail, tool options, warnings, performance, selection, box, help, context menu.

Новая схема:

- top-left: dimension + version + coordinates;
- top-center: mode switcher `World / Build / Paint / Analyze`;
- top-right: `View` menu + environment quick toggle + help `?`;
- bottom-center: floating tool rail текущего режима (только инструменты, без undo/redo);
- bottom-left: performance — только в advanced/debug, сворачиваемая;
- bottom-center над tool rail: selection/box info;
- bottom-right: help `?`, показывается при первом запуске или по запросу;
- контекстное меню — без изменений.

Убрать постоянную подпись «WASD move · RMB look…»; заменить на `?`/F1.

### 9.8. Референсы и общие выводы

Изучены: **Figma UI3, Framer, Spline, Blender, Blockbench, DaVinci Resolve, Amulet, Axiom**.

| Референс | Берём | Не берём |
| --- | --- | --- |
| **Figma UI3** | Docked resizable panels, нижний tool rail, группировка свойств, minimize UI, actions/command palette | Collaboration-хром, терминологию дизайн-инструмента, скрытие всего UI по умолчанию |
| **Framer** | Чистый top bar, переключаемые контексты слева, контекстная правая панель, нижний canvas bar, Cmd/Ctrl+K | Publish-CTA, responsive breakpoints, лишние вкладки |
| **Spline** | Сцены + outliner, stage tabs, настройки сцены при пустом выборе, Environment/Fog/Sky справа | AI Agent на первом плане, мультисцену как главную модель |
| **Blender** | Workspace tabs, viewport header, Outliner + Properties, F/Home, keyboard-first | Модальную сложность, мелкие иконки без подписей, серую эстетику |
| **Blockbench** | Режимы Edit/Paint/Animate, UV/Textures слева, Outliner справа, статус-бар, сворачивание сайдбаров | Пустой чёрный viewport, старомодные контролы |
| **DaVinci Resolve** | Постоянные page tabs, viewer + timeline + inspector, контекстные toolbar-ы, saved workspace | Плотность «всё на экране», дублирование действий |
| **Amulet** | World tree, dimensions, chunk operations | Крошечный toolbar, равный вес всех элементов, устаревшую эстетику |
| **Axiom** | 3D-камера, dockable windows, Tool Options, Active Block, History | Перегруз dock-окнами, мелкие настройки без объяснений |

Источники: Figma Blog (UI3), Framer Academy/editor screenshot, Spline Docs, Blender Manual, Blockbench Wiki, Blackmagic Design, Amulet, Modrinth/Axiom. Изображения — только исследовательские референсы, не для редистрибуции.

Общие выводы:

- **Canvas-first, но с постоянным каркасом.** Центр — мир; панели не плавают и не перекрывают canvas.
- **Режимы вместо свалки.** Один режим = один набор инструментов и одна логика правой панели.
- **Контекстный инспектор.** Пусто — мир/окружение; выбран блок — его состояние; активен инструмент — его параметры.
- **Верхняя панель — только уровень приложения.** Инструменты живут у canvas, а не в шапке.
- **Прогрессивное раскрытие.** Плотность для профи — в advanced; базовый экран понятен новичку.
- **Keyboard-first + видимые действия.** Shortcuts и command palette не заменяют кнопки, а ускоряют их.
- **Честные состояния.** Empty/loading/error/disabled объясняют, что произошло и что делать.

### 9.9. Целевой каркас

```text
┌────────────────────────────────────────────────────────────────────┐
│ Top bar: brand | Project Edit View AI | project + version + dirty  │
│          undo/redo | save | Ctrl+K | settings | window controls     │
├──────────────┬───────────────────────────────────┬─────────────────┤
│ World        │ Viewport                          │ Inspector       │
│ Project      │  mode rail bottom-center          │ Toolbox         │
│              │  HUD: dim/coords, View, env       │                 │
├──────────────┴───────────────────────────────────┴─────────────────┤
│ Blocks | Layers | Analysis | Activity                              │
├────────────────────────────────────────────────────────────────────┤
│ Status bar: core | version | dimension | selection | coords | FPS  │
└────────────────────────────────────────────────────────────────────┘
```

Правила:

- Слева — `World` (измерения, структуры, регионы) и `Project` (source, компоненты, каталог). Не дублируем `Layers`: редакторские слои живут в нижней вкладке.
- Справа — `Inspector` и `Toolbox` как две вкладки одной панели.
- Снизу — ровно 4 вкладки: `Blocks`, `Layers`, `Analysis`, `Activity`.
- Панели resizable, collapsible; размеры сохраняются per project + глобальный default.
- Минимальное окно — 1280×800; при меньшем — панели сворачиваются, canvas сохраняется.
- Top bar не содержит активный блок, дубли core status и лишние разделители.
- Status bar — единственное место для core/version/coords/FPS.

### 9.10. Режимы

| Режим | Задача | Tool rail | Toolbox | Inspector по умолчанию | Bottom focus |
| --- | --- | --- | --- | --- | --- |
| **World** | Навигация, выделение, инспекция | Select, measure, bookmark, camera | World tools | World / Selection | Layers |
| **Build** | Ставить, заменять, заливать | Place, fill, line, brush, wand | Build tools | Tool / Selection | Blocks |
| **Paint** | Терраформинг, биомы, heightmap | Terrain, biome, heightmap, smooth | Paint tools | Tool / World | Blocks |
| **Analyze** | Валидация, карты, observation | Validate, map, screenshot, inspect | Analysis tools | Selection / World | Analysis |

Правила:

- Переключение `1–4`; режим меняет tool rail, Toolbox и дефолт Inspector, но не состояние проекта.
- Долгие операции не отменяются при смене режима; прогресс остаётся в `Activity`.
- Недоступные для версии инструменты скрыты или disabled с причиной.
- Нижние вкладки не дублируют инструменты; режим может сфокусировать вкладку, но не создаёт новую.
- `Build` и `Paint` не смешивают кисти и выделение в одном rail.

### 9.11. Inspector и Toolbox

`Inspector` — три контекста, авто-переключение по действию, с возможностью закрепить:

- **Tool** — параметры активного инструмента: радиус, режим, маска, shape, replace, preview.
- **Selection** — блок/состояние, координаты, chunk, bounds, clipboard, несколько объектов.
- **World** — окружение (sky/sun/time/weather/fog), измерения, проект, каталог, save state.

`Toolbox` — библиотека инструментов:

- категории: Terrain, Heightmap, Biomes, Generators, Templates, Objects, Parametric, Build DSL;
- поиск, pinned/favorites, описание, требования (selection/assets/version), badges;
- только инструменты, доступные в текущем режиме и версии;
- выбор инструмента переключает Inspector на `Tool`, но не крадёт фокус у selection.

Окружение:

- полный контроль — в `Inspector → World → Environment`;
- быстрый popover в viewport — только shortcut (день/ночь/погода), без второй полноценной панели;
- изменения view-only, если явно не нажать `Apply to project`.

### 9.12. Визуальный язык: shadcn по умолчанию

Единственный источник стиля — штатная shadcn-тема проекта, без собственного брендинга поверх неё:

- `components.json`: `style: base-nova`, `baseColor: neutral`, `cssVariables: true`, `iconLibrary: lucide`.
- CSS-переменные в `apps/desktop/src/styles/globals.css`: `--background`, `--card`, `--popover`, `--muted`, `--border`, `--primary`, `--accent`, `--ring`, `--radius`.
- Шрифты — Geist / Geist Mono; координаты и ID — `font-mono`, tabular numbers.
- Light/dark — стандартный `.dark`-класс и те же переменные; тема приложения не хардкодится в компонентах.

Правила:

- Сгенерированные `components/ui/*` не перекрашиваем, не форкаем и не переписываем под «свой» вид.
- Никакой отдельной палитры «графит + янтарь»: цвет берётся из shadcn-переменных. Если понадобится бренд-акцент, меняется значение CSS-переменной темы, а не каждый компонент.
- Плотность — штатные размеры и варианты shadcn (`default`, `sm`, `icon`, `ghost`), а не ручные высоты; compact — через size-варианты.
- `className` — только для layout: ширина, высота, позиция, flex/grid. Не переопределять цвет, границы, радиусы и типографику сгенерированного компонента.
- Иконки — lucide, размеры как в shadcn (`size-4` и т.п.).
- Кастомный вид допустим только там, где shadcn не покрывает: 3D-canvas, HUD поверх viewport, tool rail, иконки блоков. Они используют те же переменные, шрифт и радиусы.
- Никакого glassmorphism, градиентов, нестандартных теней и «мёртвых» серых кнопок.
- Основной цвет даёт viewport (небо, свет, блоки); chrome остаётся нейтральным.

### 9.13. Клавиатура и command palette

| Действие | Shortcut |
| --- | --- |
| Command palette | `Ctrl+K` |
| Режимы World/Build/Paint/Analyze | `1` / `2` / `3` / `4` |
| Focus selection | `F` |
| Frame all | `Home` |
| Go to coordinates | `Ctrl+G` |
| Pan | `Space` + drag / middle mouse |
| Orbit / look | `RMB` / `Alt+LMB` |
| Transform move/rotate/scale | `G` / `R` / `S` |
| Brush | `B` |
| Save | `Ctrl+S` |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Y` |
| Cancel / Deselect | `Esc` |
| Help | `F1` / `?` |

Command palette умеет: переключить режим, открыть инструмент, перейти к координатам, включить grid/borders, открыть Environment, сохранить/экспортировать, открыть Settings, переключить проект. В текстовых полях shortcuts не перехватываются.

### 9.14. Состояния, motion, accessibility, приёмка

- Empty state: объяснение + одно основное действие.
- Loading: skeleton панели, progress для долгих операций, Cancel.
- Error: причина + что делать + Retry + Open logs.
- Disabled: причина в tooltip, не «просто серое».
- Read-only: явный badge и объяснение, какие действия недоступны.
- Motion: 120–180 ms, только для появления панелей/попover; `prefers-reduced-motion` отключает.
- Accessibility: focus-visible, keyboard tabs/menus, aria-labels, контраст AA, масштаб UI 100–150%.

Приёмка UI:

- [ ] Нет `Active palette block` и дублей статусов в top bar.
- [ ] Слева `World/Project`, справа `Inspector/Toolbox`, снизу 4 вкладки.
- [ ] 4 режима меняют tool rail, Toolbox и дефолт Inspector.
- [ ] Inspector переключает Tool/Selection/World и не теряет selection.
- [ ] Environment полностью в Inspector, быстрый popover — только shortcut.
- [ ] Панели resizable/collapsible, layout сохраняется.
- [ ] Command palette и shortcut-карта работают, не ломая ввод текста.
- [ ] Empty/error/disabled/read-only состояния объяснены.
- [ ] Визуальный QA: компоненты выглядят как штатный shadcn, light/dark работают, контраст AA, плотность переключается штатными size-вариантами.
- [ ] 1280×800 остаётся рабочим.

### 9.15. shadcn-first, default-first

Правило для **любой** UI-работы, до написания кода:

1. Открыть `shadcn` skill/MCP и проверить, есть ли готовый компонент под задачу.
2. Посмотреть `components.json`, установленные компоненты и `apps/desktop/src/components/ui`; не дублировать то, что уже есть.
3. Поиск по registry: `button`, `dialog`, `dropdown-menu`, `tabs`, `popover`, `tooltip`, `resizable`, `command`, `scroll-area`, `separator`, `sidebar`, `field`, `input`, `select`, `switch`, `checkbox`, `sheet`, `collapsible` и т.д.
4. Если компонент есть — использовать его **как есть**, с дефолтным стилем и штатными вариантами. Не форкать и не создавать стилевую обёртку.
5. Файлы `components/ui/*` считаются vendored: не редактировать вручную. Обновление — только через shadcn CLI/registry.
6. `className` — только layout (размер, позиция, flex/grid). Цвет, границы, радиусы и типографику компонента не переопределять; для состояний использовать штатные `variant`/`size`.
7. Если точного компонента нет — собрать custom на тех же primitives (Radix/Base UI), в том же визуальном языке shadcn, с клавиатурой и aria. Не изобретать второй дизайн.
8. Тема — только через CSS-переменные shadcn (`globals.css`), без локальных палитр в компонентах.
9. После добавления/изменения — прогнать `get_audit_checklist` и зафиксировать результат.
10. В коммите/описании задачи писать: `shadcn: <component>` или `no shadcn match; custom <name> because <reason>`.

Запрещено без причины писать свои `Button`, `Dialog`, `Menu`, `Tabs`, `Popover`, `Tooltip`, `Resizable`, `Command`, `ScrollArea`, `Separator`, `Sidebar` — сначала shadcn. Запрещено перекрашивать и стилизовать сгенерированные компоненты под «свой» вид: дефолтный shadcn — это и есть дизайн.

---

## 10. Чекпоинты: что это и зачем

### 10.1. Как сейчас

- Чекпоинт — **полный snapshot мира** на текущей ревизии, записанный в `changes/checkpoint-*.fwl`, плюс metadata v2 (`checkpointSchemaVersion`, `profileId`, `label`, `reason`, `editorState`).
- Хранится до **5** последних чекпоинтов; старые удаляются.
- Создаётся вручную (или при autosave с reason).
- Восстановление заменяет открытый мир и `editorState` (слои, scene, parametric) в памяти и помечает проект изменённым; чтобы закрепить, нужно сохранить проект.
- Исходный мир/imported source не изменяется.
- Metadata привязана к `profileId` проекта; чекпоинт другой версии, схемы или с несовпадающим checksum не открывается.

### 10.2. Чем чекпоинт не является

- Это **не Undo/Redo**. Undo — до 100 операций в памяти, теряется при перезапуске.
- Это **не autosave**. Autosave — каждые 30 секунд, один файл для crash recovery.
- Это **не backup проекта целиком**. Project settings, components и source copy живут отдельно.
- Это **не замена git/архиву**. Для внешнего backup нужен Export project.

### 10.3. Зачем он нужен

- Точка восстановления перед рискованной массовой операцией (fill/delete/terrain/import).
- Возможность откатиться, когда Undo уже не хватает или приложение перезапускалось.
- Быстрый “save point” перед сменой версии, импортом структуры, экспериментами.
- Диагностика: сравнить состояние до/после.

### 10.4. Что улучшить

- Автоматически создавать checkpoint перед:
  - delete/fill/replace большого объёма;
  - импортом структуры/мира;
  - сменой версии проекта;
  - массовыми generator/terrain операциями.
- Добавить label/note и показывать reason, revision, размер, дату.
- Сохранять `editorState` (layers/scene/parametric) вместе с миром.
- Разрешить pin до N чекпоинтов, чтобы авто-чекпоинты не вытесняли важные.
- Восстанавливать **как копию** (не заменять текущий мир) с явным выбором.
- Показать в UI короткое объяснение: «Checkpoint — полная точка восстановления мира, не Undo».
- Экспорт чекпоинта в отдельный проект/архив — опционально.

---

## 11. Сцена: окружение, свет, сетка, навигация

### 11.1. Главное правило

`ViewportRenderer` создаётся только когда:

- проект открыт и `READY`;
- компоненты (Java/assets) разрешены;
- мир загружен или явно показан loading state.

До этого — Welcome/Wizard/Preparing. Никаких `createDemoSnapshot`, `world_info`, `render_snapshot` и `resume()` до проекта.

### 11.2. Окружение

Добавить `SceneEnvironment`:

- sky dome / gradient;
- sun и moon, положение по времени;
- stars ночью;
- clouds (опционально, без тяжёлой симуляции);
- ambient/hemisphere light;
- directional sun/moon light;
- fog, зависящий от render distance, времени и погоды;
- dimension presets:
  - Overworld — голубое небо, солнце, облака;
  - Nether — красный туман, без неба;
  - End — тёмно-фиолетовое небо.

UI:

- полный контроль — `Inspector → World → Environment`:
  - время: dawn / day / dusk / night + slider (0–24000);
  - auto cycle on/off;
  - weather: clear / rain / thunder;
  - fog density;
  - `Apply to project` (view-only, не меняет world data).
- быстрый popover в viewport — только shortcut для day/night/weather/fog; он не дублирует полную панель.

### 11.3. Освещение

Сейчас `MeshBasicMaterial` — сцена не освещена. План:

**Этап A — базовое окружение:**

- `MeshLambertMaterial`/custom material с ambient + directional;
- face shading как в Minecraft (top/side/bottom);
- fog и sky;
- без динамических теней (Minecraft их не использует).

**Этап B — Minecraft-like light:**

- расширить `RenderSnapshot`/`ChunkSnapshot` полями `SkyLight`/`BlockLight` (если есть в chunk NBT);
- vertex attributes: sky light, block light, AO;
- shader: `max(skyLight * daylight, blockLight) * faceShade * AO`;
- если light data отсутствует — приблизительный fallback (top-down skylight) с явной пометкой;
- emissive blocks (lava/glowstone/sea lantern) — минимальный вклад;
- сохранить cutout/translucent.

**Этап C — полировка:**

- biome tint для grass/leaves/water (вместо фиксированных цветов);
- анимация воды/лавы — опционально;
- weather particles;
- screenshot metadata: time/weather/environment.

### 11.4. Сетка и линии

Заменить `GridHelper(256, 256)` и жёсткие границы:

- адаптивная сетка в world space:
  - 1 блок — тонкая;
  - 16 блоков — chunk;
  - 256 блоков — major;
- fade по расстоянию;
- оси X/Z с цветами;
- корректный depth, без z-fighting;
- grid plane на y=0 или minY выбранного измерения (настраиваемо);
- границы чанков — по реальным minY/maxY измерения, только вокруг загруженных чанков;
- опционально границы секций;
- толщина линий 1px, без «жирных» артефактов;
- настройки grid/borders в View menu, сохраняются в проекте.

### 11.5. Камера и навигация

Сейчас есть fly и orbit, но управление разрознено. Сделать единый `CameraRig`:

- режимы: Fly, Orbit, Top-down;
- Fly: WASD, Space/Ctrl, Shift boost, RMB look, pointer lock;
- Orbit: rotate/pan/zoom, double-click focus, pivot на selection;
- Top-down: планировочный вид, pan/zoom;
- `F` — focus selection;
- `Home` — frame all;
- `Ctrl+G` — go to coordinates;
- speed indicator + настройка;
- camera bookmarks (опционально);
- сохранение/восстановление камеры per project;
- плавное сглаживание;
- help dialog вместо постоянной подписи.

### 11.6. Производительность

- Lazy asset atlas; кэш по версии.
- Mesh budget уже есть; сохранить.
- Frustum culling/fog.
- Не грузить все измерения сразу; только выбранное + соседние по необходимости.
- Performance panel — advanced/debug, не постоянный элемент.
- Benchmark на reference machine; фиксировать budget.

---

## 12. Что ещё стоит поправить

1. **Динамические измерения.** `WorldTree` должен брать dimensions из `WorldInfo`, а не из трёх констант; Overworld/Nether/End не всегда “loaded”.
2. **Версия в UI.** Показывать версию проекта в top bar/status bar и Project Settings; убрать глобальную «Minecraft 1.21.4» из Settings.
3. **Удаление и перенос проекта.** Сейчас UI не умеет; добавить delete (Recycle Bin), relocate, duplicate, export archive.
4. **Rename.** Сейчас rename меняет только display name; либо явно писать «Display name», либо добавить безопасное переименование папки.
5. **Save state.** Оставить один индикатор (top bar/project badge), убрать дубли из performance panel/Project Manager.
6. **Autosave.** 30 секунд — приемлемо, но добавить настройку/индикатор и checkpoint перед рискованными операциями.
7. **Derived data.** Четко показывать `Preserve`/`Vanilla rebuild` при экспорте; не обещать пересчёт.
8. **Block entities/entities.** Предупреждать при импорте/экспорте структур и размещении сундуков/табличек.
9. **World export UI.** Добавить one-click, destination picker, verification report.
10. **Version-aware structure origin.** `defaultOriginY` сейчас захардкожен; брать из профиля/измерения.
11. **Resource pack validation.** Формат pack_format под версию, а не фиксированный 46.
12. **Доступность.** Focus-visible, клавиатурная навигация, aria для вкладок/меню, reduced motion.
13. **Пустые состояния.** Везде объяснять, что нужно сделать, а не просто «нет данных».
14. **Ошибки.** Единый error-presenter: причина, что делать, Retry, Open logs.
15. **Документация.** Обновить README/reference/formats/architecture/changelog, убрать устаревшие «only 1.21.4» после расширения.
16. **Каталог контента.** Палитра должна показывать все блоки и предметы выбранной версии, включая technical/creative-only, с состояниями, категориями и понятными ограничениями.
17. **UI-система.** Ввести режимы, shadcn-каркас, command palette и контекстный инспектор вместо латания старого chrome; см. §9.8–9.15.

---

## 13. Этапы внедрения

> Порядок учитывает приоритет: сначала автоскачивание и нормальный первый запуск, но архитектурный фундамент версии нужен до него.

### P0. Фундамент и первый запуск

**Результат:** проект 1.21.4 создаётся через новый wizard с auto Java/assets; baseline-каталог блоков/предметов 1.21.4 собран из client JAR; ручной путь работает; сцена не создаётся без проекта; настройки не дублируют мастер.

- Новый `minecraft-version` crate + `VersionProfile`/`VersionCatalog`.
- Project schema v2 + snapshot v3 + checkpoint v2; старые схемы не читаются, миграций нет.
- Единый component manager (Java/assets/server), progress/cancel/verify.
- Welcome screen + Project Gate.
- Мастер создания: Basics → Version → Source → Components → Review → Preparing.
- Убрать `Active palette block` из toolbar.
- Убрать viewport до проекта.
- Убрать дублирующие setup-блоки из Settings (оставить inventory).
- Быстрая чистка нижней панели: удалить `Tools`/`Warnings`, сгруппировать Analysis/Activity (полный перенос инструментов можно в P4).

**Exit criteria:**

- Новый пользователь создаёт проект без похода в Settings.
- Java/assets скачиваются с проверкой и прогрессом; manual/placeholder работают.
- Нет `world_info`/renderer до проекта.
- `Settings` не содержит второго мастера создания.
- Миграция старых проектов проходит, тесты зелёные.

### P1. Modern multi-version

**Результат:** full native для 1.18–1.21.x + 26.1–26.3 (по готовности split); полный каталог блоков/предметов каждой версии; палитра и ассеты совпадают с версией.

- Modern chunk/biome codec.
- Dynamic block catalog per version.
- Client JAR download + asset pipeline per version.
- Version-aware server generation.
- Import/edit/export/generate для каждой версии матрицы.
- Version upgrade (same-family) через копию.
- Version badges/capabilities в UI.

**Exit criteria:**

- Матрица версий проверена фикстурами.
- Для каждой full-версии: create → edit → save → reopen → export → verify.
- Assets и палитра совпадают с версией.
- Генерация создаёт мир правильной версии.

### P2. 1.13–1.17

**Результат:** full native для flattened classic Anvil и полный каталог блоков/предметов 1.13–1.17.

- Chunk/biome adapters.
- Version-specific model/blockstate differences.
- Server properties/level-type/datapack format.
- Структуры/экспорт под версию.
- Тесты и фикстуры.

**Exit criteria:** тот же end-to-end набор, что P1, для 1.13–1.17.

### P3. Legacy и форматы

**Результат:** 1.8–1.12 native/convert, pre-1.8 convert/passthrough, legacy catalog mapping, split 26.1+, структуры.

- Numeric ID + metadata codec.
- McRegion read.
- Official server upgrade path.
- `.schem`, `.schematic`, `.litematic` (read).
- World zip import/export; project archive.
- Snapshot policy.

**Exit criteria:**

- Legacy world импортируется и либо редактируется, либо конвертируется с отчётом.
- Passthrough сохраняет байты.
- Структуры round-trip.
- Нет ложных «full support» бейджей.

### P4. UI/UX и сцена

**Результат:** production-ready оболочка, 4 режима, контекстный Inspector и нормальная 3D-сцена.

- Нижняя панель 4 вкладки; Toolbox справа.
- Режимы World/Build/Paint/Analyze, контекстный Inspector, command palette, design tokens.
- Project Settings, delete/relocate/duplicate.
- Top bar/menus/HUD cleanup.
- SceneEnvironment: sky/sun/moon/fog/weather/time.
- Lighting: Lambert → baked light/AO.
- Adaptive grid/chunk borders.
- CameraRig: fly/orbit/top-down, focus/frame/bookmarks.
- Resizable panels, persisted layout, accessibility.

**Exit criteria:**

- Нет переполненных вкладок и дублирующих контролов.
- Сцена выглядит как Minecraft-редактор, а не серая пустота.
- День/ночь/погода/сетка/камера работают и сохраняются.
- UIScale и оконные controls не сломаны.

### P5. Production hardening

- Полная матрица тестов и фикстур.
- Security review загрузок/архивов/конвертаций.
- Performance budget.
- Docs/ADRs/changelog.
- Release checklist, manual Tauri QA.
- Known limitations обновлены.

---

## 14. Тестирование и качество

### 14.1. Версии и форматы

- Unit-тесты `VersionProfile`/`VersionCatalog`: каждая версия имеет профиль и capabilities.
- Golden fixtures для каждого chunk family:
  - synthetic NBT + реальные маленькие миры, созданные official server;
  - pre-1.13, 1.13–1.17, 1.18+, split.
- Round-trip: decode → encode → decode, byte-preservation untouched fields.
- Import → edit → export → reopen → compare.
- Generation → import → verify.
- Version upgrade → original untouched, converted copy verified.
- Catalog snapshot tests для каждой full-версии: counts, известные блоки/предметы, валидность состояний, отсутствие дублей.
- Catalog diff tests: added/removed/renamed между версиями, изменения тегов и категорий.
- Legacy mapping: numeric ID + metadata → named ID, round-trip и отчёт unknown.
- Unknown imported states сохраняются и не подменяются.

### 14.2. Компоненты и загрузки

- Mock HTTP: success, 404, timeout, redirect to disallowed host, checksum mismatch, wrong size, cancel.
- Disk-space failure, partial file cleanup, atomic rename.
- Manual path: symlink, wrong architecture, wrong version, unreadable.
- Offline cache fallback.
- Host allowlist tests.
- Archive traversal/zip bomb tests.

### 14.3. UI

- Welcome → wizard → preparing → editor state machine.
- No renderer/world calls before project.
- Manual/auto/placeholder branches.
- Component error/retry.
- Version support badges and disabled reasons.
- Bottom tabs: only 4; tool panels present in Toolbox.
- Modes World/Build/Paint/Analyze switch tool rail, Toolbox and Inspector defaults without losing selection.
- Inspector switches Tool/Selection/World; Environment lives in World.
- Command palette and shortcuts work without hijacking text inputs.
- shadcn audit checklist проходит; сгенерированные `components/ui/*` не изменены вручную; custom-компоненты — только для gap-ов, в shadcn-языке и с a11y-тестами.
- Catalog palette: all blocks/items of version, categories/search/filters, technical/creative-only badges, state picker.
- Top bar: no active block badge, window controls still work.
- Project Settings: version upgrade, relink, delete/relocate.
- Accessibility: keyboard tabs/menus, focus, labels.

### 14.4. Сцена

- Environment presets, time/weather transitions.
- Grid/borders at different dimensions/heights.
- Camera modes and focus/frame.
- Lighting fallback when light data missing.
- Performance budget on reference GPU.
- Capture metadata includes environment/time.

### 14.5. CI guardrails

- Grep-тест: `1.21.4`/`4189` не должны быть захардкожены вне version catalog/tests.
- Matrix snapshot test: supported claims совпадают с capabilities.
- Catalog parity test: для каждой full-версии каталог содержит ожидаемые блоки/предметы и валидные состояния.
- Test that full-version palette reads the catalog, not the curated registry; curated — только fallback/quick picks.
- Test that Settings has no setup duplication (component inventory only).
- Test that viewport is not mounted without project.
- Grep-тест: нет `migration`/`deprecated`/`compat`/`_old` кода и чтения старых внутренних схем; demo и фикстуры — на актуальной схеме.
- Cargo clippy/fmt + pnpm typecheck/lint/format/test.

### 14.6. Ручной QA

- Реальный Tauri: window controls, drag, maximize/restore, resize, UIScale 100%.
- Download real Java/client/server for 2–3 versions.
- Generate real worlds on 1.16.5, 1.18.2, 1.21.4.
- Проверить палитру на 1.16.5/1.18.2/1.21.4: все блоки/предметы, состояния, technical/creative-only, unknown.
- Import/export with official server.
- Offline/relink scenarios.
- Long-running generation/cancel/kill.

---

## 15. Документация и ADR

Обновить:

- `README.md` — новые версии, компоненты, первый запуск.
- `docs/getting-started.md` — новый wizard.
- `docs/reference.md` — support matrix, requirements, known limitations.
- `docs/formats.md` — все форматы и что реально пишется.
- `docs/catalog.md` — блоки/предметы по версиям, категории, states, technical/creative-only, ограничения.
- `docs/ui.md` — каркас, режимы, Inspector/Toolbox, shortcuts и shadcn-тема по умолчанию.
- `docs/architecture.md` — ADR:
  - ADR-001 заменить на version profiles;
  - ADR-001a — content catalog: blocks/items per version;
  - ADR-004 — asset download/manual/placeholder;
  - ADR-007 — structure formats;
  - ADR-008 — checkpoints vs undo/autosave;
  - ADR-014 — creation modes;
  - ADR-015 — component provisioning;
  - ADR-016 — UI shell, modes and contextual inspector;
  - ADR-017 — shadcn-first and custom component policy;
  - ADR-018 — greenfield schema reset, no migrations or legacy code.
- `docs/advanced-workflow.md` — Toolbox, scene, version conversion.
- `CHANGELOG.md` — по этапам.
- `docs/legal/third-party-notices.md` — если добавляются новые зависимости.

---

## 16. Риски и меры

| Риск | Почему | Мера |
| --- | --- | --- |
| Огромный объём version adapters | У каждой семьи свой chunk/biome/NBT | Семейства + фикстуры, поэтапно; не обещать full до тестов |
| Старые версии и Java | Java 8/16/17/21, разные server flags | Профили Java/server; manual fallback; отключение generation, если нет JAR |
| Конвертация миров | DataFixerUpper медленный/неполный, downgrade невозможен | Только upgrade, копия, checkpoint, отчёт, без downgrade |
| Ассеты старых версий | Нет blockstate/models до 1.8; другие пути | Fallback texture resolver; pre-1.8 read/convert, не full visual |
| Размер каталога | 10k+ блоков/предметов, иконки и состояния | Виртуализация, ленивый atlas, поисковый индекс, бюджет памяти |
| Дрейф каталога между версиями | Блоки/предметы добавляются, удаляются и переименовываются | CatalogDiff, aliases, fixtures, явные unknown |
| Технические и creative-only блоки | Их легко принять за обычные | Флаги, бейджи, правила размещения, предупреждения |
| Скачивание и лицензии | Mojang assets нельзя бандлить | Только official URLs, checksum, user consent, no redistribution |
| Безопасность архивов/JSON | Недоверенные JAR/zip/version JSON | Allowlist, bounds, traversal/symlink checks, fuzz |
| Производительность света | Per-vertex light/AO дороже | Этапы A/B, budget, fallback |
| UI scope creep | Много идей | P0 быстрый результат, затем измеримые этапы |
| Соблазн тащить старые схемы | Кажется, что миграции «пригодятся» | Пользователей и проектов нет; v1 удаляется сразу, compatibility-код запрещён (§3.1), CI-grep |
| Потеря данных | Конвертация/экспорт | Source immutable, staged writes, verify, checkpoints, Recycle Bin |

---

## 17. Non-goals

- Bedrock Edition.
- Полное семантическое редактирование block entities/entities (сундуки, таблички, мобы).
- Полное редактирование item components/NBT; каталог и выбор предметов поддерживаются, данные сохраняются.
- Пересчёт heightmaps/light/POI на стороне Rust.
- Гарантированная поддержка модифицированных миров/кастомных измерений и registry блоков/предметов.
- Полная симуляция Minecraft (физика, редстоун, игровой цикл).
- Динамические тени, шейдеры уровня Optifine.
- Облачная синхронизация/совместная работа.
- Downgrade версий мира.

---

## 18. Definition of Done

- [ ] Пользователь может создать проект любой full-версии через мастер, не заходя в Settings.
- [ ] Java, client JAR и server JAR скачиваются автоматически по одному выбору, с проверкой и прогрессом.
- [ ] Ручной путь и placeholder/read-only работают и явно обозначены.
- [ ] Нет ни одного renderer/world-запроса до открытия проекта.
- [ ] Нет дублирующих setup-блоков в Settings.
- [ ] Верхняя панель не показывает активный блок; оконные controls работают.
- [ ] Снизу 4 вкладки; инструменты — в Toolbox.
- [ ] 4 режима (World/Build/Paint/Analyze), контекстный Inspector и command palette работают.
- [ ] UI использует shadcn-компоненты в дефолтном виде; `components/ui/*` не перекрашены и не форкнуты; custom — только для gap-ов, с audit checklist.
- [ ] Нет миграций, compatibility shims и чтения старых внутренних схем; demo пересоздан под актуальную схему.
- [ ] Для каждой full-версии палитра содержит все ванильные блоки и предметы, включая technical/creative-only; состояния валидируются.
- [ ] Проект фиксирует `catalogId`/hash; палитра, валидация и экспорт используют один и тот же каталог.
- [ ] Unknown/removed block states сохраняются и явно помечаются, без молчаливой подмены.
- [ ] Для каждой full-версии: create → edit → save → reopen → export → verify.
- [ ] Для read-only/convert версий статус честно показан и проверен.
- [ ] Checkpoints понятны в UI, сохраняют editor state и создаются перед риском.
- [ ] Сцена: небо, свет, день/ночь, погода, сетка, камера.
- [ ] Матрица версий и known limitations обновлены.
- [ ] Тесты, typecheck, lint, format, clippy зелёные.
- [ ] Ручной Tauri QA пройден.

---

## 19. Рекомендации по объёму первого релиза

Чтобы не растянуть «всё сразу» на месяцы:

1. **Первый shippable slice:** 1.21.4 + 1.20.6 + 1.18.2 full с полным каталогом блоков/предметов, 1.13–1.17 read/convert, новый wizard, auto Java/assets, no-scene gate, 4 режима/4 вкладки, базовое окружение сцены.
2. **Второй slice:** 1.13–1.17 full, 26.1+ split, структуры `.schem`/`.schematic`.
3. **Третий slice:** legacy 1.8–1.12, pre-1.8 conversion, полный свет/AO, project delete/relocate, production polish.

Если нужен самый короткий путь к «вау»-эффекту: P0 + часть P4 (режимы, shadcn-каркас, sky/light/grid/camera) + 4 вкладки + baseline-каталог. Это уже убирает «серость» и хаос, не дожидаясь всех версий.

---

## 20. Решения и договорённости

Пункты 1–7 — рекомендации к подтверждению; 8–9 и 14 уже приняты и внесены в §5.4, §9.8–9.15 и §3.1.

1. **Диапазон первого релиза:** рекомендуем modern 1.18–1.21.x full + 1.13–1.17 convert, остальное поэтапно.
2. **Pre-1.13:** рекомендуем сначала convert/passthrough, не native edit.
3. **Snapshots:** рекомендуем read-only/convert, без гарантии записи.
4. **Bedrock:** рекомендуем out of scope.
5. **Смена версии:** рекомендуем только upgrade и только через копию с checkpoint; downgrade запрещён.
6. **Автоскачивание:** рекомендуем «auto по умолчанию, но по явной кнопке», без фоновых загрузок без согласия.
7. **Визуальный уровень сцены:** рекомендуем Minecraft-like environment + baked light/AO, без динамических теней и шейдеров.
8. **Каталог контента (принято):** все ванильные блоки и предметы точной версии, включая technical/creative-only, состояния, категории и предметы; modded/custom — passthrough и предупреждение.
9. **UI-система (принято):** 3 панели, 4 режима, 4 нижние вкладки, контекстный Inspector, Toolbox, command palette; shadcn-тема по умолчанию (neutral, CSS variables), comfortable density, без собственной палитры.
14. **Чистая перезапись (принято):** `project.json` v2, `.fwl` v3 и checkpoint v2 — единственные схемы. Миграций, legacy-кода и compatibility-fallback-ов нет; старый код удаляется сразу. Legacy Minecraft-версии остаются продуктовой фичей, а не оправданием для старого кода.

После подтверждения этих пунктов можно начинать с P0: version profile + component manager + wizard + no-scene gate. Это даст самый большой пользовательский эффект и не потребует переписывать всё приложение сразу.

---

## 21. Рабочий чеклист реализации

> Этот раздел — внутренний трекер. Отмечать `[x]` только когда задача реально сделана и проверена (код + тесты/typecheck/lint/format, где применимо). Выполненные пункты не удалять: по ним видно, где мы находимся.
>
> Чеклист покрывает весь объём, включая все семейства версий. Если какое-то решение из §20 изменится, соответствующие пункты корректируются, а не молча вычёркиваются.

**Текущий фокус:** `P0-46` — Review: сводка + все предупреждения + ожидаемый размер/время. Статус: P0 в работе.

**Следующий шаг после закрытия текущего фокуса:** `P0-47`. Если пункт распадается на несколько шагов, он всё равно остаётся одним до полной проверки.

### 21.0. Правила ведения

- В начале работы открывать этот раздел, читать «Текущий фокус» и первый неотмеченный пункт.
- Один пункт = один проверяемый результат. Не отмечать `[x]`, пока не выполнены его подпункты и проверки.
- После закрытия пункта: поставить `[x]`, обновить «Текущий фокус», добавить строку в журнал §21.8.
- Коммит делать после логического блока (не после каждого файла). Сообщение коммита — по Conventional Commits, в теле указывать закрытые ID.
- Push — только после отдельного явного разрешения. Локальные коммиты не пушить.
- Если обнаружен блокер: не обходить молча, а добавить подпункт `(blocked)` с причиной и обновить «Текущий фокус».
- Перед коммитом блока: `cargo test`, `cargo clippy`, `cargo fmt --check`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check`, `pnpm test` (или их актуальные аналоги из `package.json`).
- Перед UI-задачей: проверить shadcn через skill/MCP (§9.15); после UI-изменений — прогнать shadcn audit checklist и записать результат.
- Не писать миграции и compatibility shims: приложение нигде не установлено, старые внутренние схемы не поддерживаются (§3.1). Старый код удаляется в том же изменении.
- Документацию обновлять в том же блоке, где меняется поведение, а не «потом».

### 21.1. Сделано до старта

- [x] `DONE-01` Объединить брендинг, меню и оконные контролы в один toolbar; нормализовать диалоги; убрать дублирующий core status. Коммит `b80dbcc`.
- [x] `DONE-02` Почистить хрупкие TypeScript-тесты, перевести проверки на наблюдаемое поведение и semantic roles. Коммит `d7ae9a5`.
- [x] `DONE-03` Провести аудит версий, компонентов, нижней панели, viewport, чекпоинтов и загрузок.
- [x] `DONE-04` Написать план и этот чеклист.
- [x] `DONE-05` Изучить UI-референсы (Figma, Framer, Spline, Blender, Blockbench, DaVinci, Amulet, Axiom) и зафиксировать решения в §9.8–9.14.

### 21.2. P0 — фундамент и первый запуск

**Цель:** проект 1.21.4 создаётся через новый wizard с auto Java/assets; ручной путь работает; сцена не создаётся без проекта; Settings не дублируют мастер.

#### A. Version foundation

- [x] `P0-01` Создать crate `crates/minecraft-version`, подключить в workspace, без зависимостей на Tauri/UI.
- [x] `P0-02` `VersionId`, `VersionFamily`, `ReleaseType`: парсинг, сравнение, serde; тесты на `1.21.4`, `1.21`, `26.1-snapshot-3`.
- [x] `P0-03` `VersionProfile` со всеми полями из §5.1 и `serde(rename_all = "camelCase")`.
- [x] `P0-04` `SupportLevel` (`full`, `read-write-subset`, `read-only`, `convert`, `out-of-scope`) и `WorldCapabilities`.
- [x] `P0-05` Базовый профиль `java-classic-1.21.4`: DataVersion 4189, Java 21, pack 46, datapack 61, dimensions/height, named block/item catalog.
- [x] `P0-06` `VersionCatalog`: загрузка и кэш `version_manifest_v2.json`, TTL, offline fallback, allowlist.
- [x] `P0-07` Загрузка version JSON: `javaVersion.majorVersion`, client/server downloads, SHA-1.
- [x] `P0-08` Сборка `VersionProfile` из семейства + manifest + внутреннего `version.json` JAR.
- [x] `P0-09` Тесты: каждый профиль имеет capabilities; нет противоречий `full` ↔ отсутствие write.
- [x] `P0-10` Убрать глобальные `TARGET_MINECRAFT_VERSION`/`TARGET_DATA_VERSION` из рабочего пути; дефолт остаётся только для тестов/пустого проекта.
- [x] `P0-11` CI-grep: `1.21.4` и `4189` встречаются только в каталоге версий, фикстурах и тестах.

#### B. Project data

- [x] `P0-12` `project.json` schema v2: `target` + `components` (см. §5.3), serde + validation.
- [x] `P0-13` Удалить schema v1 целиком: reader/writer, миграцию, backup-путь и тесты старой схемы; `project.json` существует только в v2.
- [x] `P0-14` Snapshot `.fwl` v3: `profileId`, `dataVersion`, layout, biome mode.
- [x] `P0-15` Удалить snapshot v2 reader/writer и тесты v2; пересоздать `examples/furybuilder-demo.furybuilder` под schema v2 / `.fwl` v3 и обновить `demo_project` тест.
- [x] `P0-16` Checkpoint metadata v2: `profileId`, `label`, `reason`, `editorState`; v1 не читается, compatibility-полей нет.
- [x] `P0-17` `world_info`, project summary и MCP metadata возвращают версию проекта, а не глобальную константу.

#### C. Component manager

- [x] `P0-18` Модель компонентов: kind `java|assets|server`, mode `managed|manual|placeholder`, status DTO.
- [x] `P0-19` Facade `components_*` (§6.1): status/resolve/install/cancel/relink/remove/verify.
- [x] `P0-20` Java: подбор Temurin по major через Adoptium API, кэш `components/java/<major>/<release>`.
- [x] `P0-21` Java: download/verify/install, SHA-256, progress, cancel, cleanup partial-файлов.
- [x] `P0-22` Java manual: `java.exe`, `-version`, major/arch/vendor; новее — warning, старее — block.
- [x] `P0-23` Assets: client JAR download по version JSON, SHA-1/размер, кэш + manifest.
- [x] `P0-24` Assets: инспекция JAR (`version.json`, blockstates, block/item models, item definitions, textures, lang, tags, atlas) и baseline-каталог 1.21.4.
- [x] `P0-25` Assets manual: `.minecraft` root / `versions/<id>/<id>.jar` / выбранный client.jar.
- [x] `P0-26` Assets placeholder: явный режим с предупреждением, не выдаётся за полноценные текстуры.
- [x] `P0-27` Server: download по точной версии, SHA-1, кэш, проверка внутреннего `version.json`.
- [x] `P0-28` EULA: consent привязан к SHA-1 JAR + версии; без consent запуск запрещён.
- [x] `P0-29` Общий pipeline: allowlist хостов, disk preflight, atomic rename, proxy, retry, offline.
- [x] `P0-30` Tauri commands + TS-клиент + события progress/error.
- [x] `P0-31` Mock HTTP-тесты: success/404/timeout/redirect/hash mismatch/wrong size/cancel/disconnect.
- [x] `P0-32` Тесты manual/placeholder/relink, включая symlink, неверную архитектуру и версию.

#### D. Lifecycle и Welcome

- [x] `P0-33` Lifecycle store: `CLOSED → PREPARING → READY → SAVING/ERROR`; единый источник состояния.
- [x] `P0-34` Welcome screen: бренд, недавние проекты, `New project`/`Open`/`Import`, статус ядра.
- [x] `P0-35` Project gate: viewport/renderer/`world_info`/`render_snapshot`/`resume` только в `READY`.
- [x] `P0-36` Teardown при закрытии: dispose renderer, reset stores, stop autosave, cancel jobs.
- [x] `P0-37` Recoverable autosave: badge в недавних проектах и восстановление из Welcome.
- [x] `P0-38` Тесты: до проекта нет ни одного world/render вызова; teardown очищает состояние.

#### E. Мастер создания

- [x] `P0-39` Каркас wizard: шаги, навигация, валидация, прогресс, полноэкранный режим.
- [x] `P0-40` Basics: имя, папка, режим Empty/Flat/Import/Vanilla, требования режима.
- [x] `P0-41` Version: каталог, поиск, latest/recommended, snapshot toggle, карточка, badges, предупреждения.
- [x] `P0-42` Source Empty/Flat: параметры слоёв/высоты/биома под версию.
- [x] `P0-43` Source Import: выбор папки/архива, анализ, blockers, `Import as-is`/`Convert`.
- [x] `P0-44` Source Vanilla: seed/preset/radius/structures/datapacks, EULA, время/диск.
- [x] `P0-45` Components: Auto/Manual/Existing для Java/assets/server с состоянием и ошибками.
- [ ] `P0-46` Review: сводка + все предупреждения + ожидаемый размер/время.
- [ ] `P0-47` Preparing: шаги, progress, Cancel/Retry; скачанное не теряется.
- [ ] `P0-48` Создание проекта через v2 manifest с logical ids и хэшами компонентов.
- [ ] `P0-49` Import создаёт проект с автоопределённой/подтверждённой версией.
- [ ] `P0-50` Убрать ProjectManager-dialog в режиме «нет проекта»; оставить открытие/переключение.
- [ ] `P0-51` UI-тесты всех веток wizard (auto/manual/placeholder, ошибки, retry).

#### F. Settings без дублей

- [ ] `P0-52` Settings: Appearance / Components / Updates / Diagnostics / Advanced.
- [ ] `P0-53` Удалить `Java & World Generation`, `DependencyReadiness`, дубли server/assets setup.
- [ ] `P0-54` Components inventory: размер, путь, verify, remove, open folder.
- [ ] `P0-55` Общие `ComponentCard`/`Picker`/`Status` для wizard и Settings.
- [ ] `P0-56` VanillaWorldWizard больше не спрашивает Java/server отдельно — использует шаг Components.
- [ ] `P0-57` Тест: в Settings нет второго мастера создания.

#### G. Быстрая чистка UI

- [ ] `P0-58` Убрать `Active palette block` и дубли core/save status из toolbar/performance.
- [ ] `P0-59` Bottom panel: `Palette → Blocks`; убрать Tools/Warnings; Analysis = Observation+Validation; Activity = Console+History.
- [ ] `P0-60` Обновить `BOTTOM_PANEL_TABS`; сохранённый layout старого формата не читаем и сбрасываем к дефолту.
- [ ] `P0-61` Тесты: Tools/Warnings удалены, Analysis/Activity сгруппированы, toolbar без активного блока; полный переход на 4 вкладки — P4.

#### H. Закрытие P0

- [ ] `P0-62` `cargo test`/clippy/fmt + `pnpm typecheck`/lint/format/test — зелёные.
- [ ] `P0-63` Ручной smoke: 1.21.4 создать auto и manual, открыть, сохранить, переоткрыть.
- [ ] `P0-64` Обновить docs: getting-started, reference, formats, changelog (P0).
- [ ] `P0-65` Коммит P0; push — только после отдельного разрешения.

### 21.3. P1 — modern multi-version

**Цель:** full native для 1.18–1.21.x + 26.1–26.3 (по готовности split); палитра содержит все блоки и предметы версии, ассеты совпадают с версией.

#### A. Codec и профили

- [ ] `P1-01` Профили 1.18–1.21.x: exact DataVersion, Java, pack/datapack, dimensions, height.
- [ ] `P1-02` Ввести `WorldCodec` trait/interfaces (§5.2), не меняя поведение 1.21.4.
- [ ] `P1-03` `ClassicAnvilCodec`: вынести 1.21.4 в реализацию + golden-тесты без diff.
- [ ] `P1-04` Параметризовать section layout, palette, heightmaps и light для 1.18+.
- [ ] `P1-05` Section biomes 1.15–1.17 + column biomes 1.13–1.14 и conversion.
- [ ] `P1-06` Version-aware chunk NBT: секции, палитры, block entities preserve.
- [ ] `P1-07` Minor adapters 1.18/1.19/1.20/1.21 (отличия полей и registry).
- [ ] `P1-08` Split layout 26.1–26.3: `dimensions/`, `players/`, split region (если готово).

#### B. Ассеты и каталог

- [ ] `P1-09` Извлекать из client JAR blockstates, block/item models, item definitions, textures, lang и tags для каталога; curated список — quick picks/fallback.
- [ ] `P1-10` Property keys/defaults/validation по blockstates; item definitions/models.
- [ ] `P1-11` Atlas + кэш `version + jarSha1 + packStackHash + catalogSchema`.
- [ ] `P1-12` Version-aware палитра: поиск, недавние, избранное, базовые категории.
- [ ] `P1-13` Unknown imported states: preserve + warning, не считать writable.
- [ ] `P1-14` Resource pack stack per project + `pack_format` validation.

#### C. Операции с миром

- [ ] `P1-15` Dimensions из профиля в domain/import/render/WorldTree.
- [ ] `P1-16` Import 1.18–1.21.x end-to-end.
- [ ] `P1-17` Export 1.18–1.21.x same-version + staged + verify.
- [ ] `P1-18` Generation 1.18–1.21.x: level-type, Java, server.properties, пресеты.
- [ ] `P1-19` Upgrade same-family через копию + checkpoint + отчёт.
- [ ] `P1-20` Support badges/capabilities и disabled-reasons в UI.

#### D. Полный каталог блоков и предметов

- [ ] `P1-21` `CatalogEntry`/`CatalogSnapshot`: id, legacyId, имя, категория, теги, иконка, aliases, states, item form, flags.
- [ ] `P1-22` `CatalogBuilder` из client JAR + official metadata; кэш по `version + jarSha1 + catalogSchema`; `catalogId` в проекте.
- [ ] `P1-23` Block states: все property keys, допустимые значения, default state, варианты и item form.
- [ ] `P1-24` Item catalog: все предметы, иконки, имена, категории, item-only/placeable; components read/preserve.
- [ ] `P1-25` Флаги `technical`, `creativeOnly`, `fluid`, `airlike`, `blockEntity`, `obtainable`, `hasItem`; правила размещения и предупреждения.
- [ ] `P1-26` Категории как creative inventory + `All`/`Recent`/`Favorites`; поиск по имени/ID/тегу; фильтры.
- [ ] `P1-27` `CatalogDiff` между версиями: added/removed/renamed, изменения states/tags; aliases и unknown.
- [ ] `P1-28` Catalog UI: виртуализированный список, 3D-иконка, state picker, бейджи `not in this version` и disabled-причины.
- [ ] `P1-29` Производительность каталога: ленивый atlas, поисковый индекс, бюджет памяти на 10k+ записей.
- [ ] `P1-30` Тесты каталога: counts, известные блоки/предметы, валидность состояний, отсутствие дублей, diff-snapshots.

#### E. Фикстуры, E2E и закрытие P1

- [ ] `P1-31` Фикстуры на каждое семейство + E2E create→edit→save→reopen→export→verify с проверкой каталога.
- [ ] `P1-32` Обновить матрицу/docs/changelog и закоммитить P1.

### 21.4. P2 — 1.13–1.17

**Цель:** full native для flattened classic Anvil; полный каталог блоков/предметов 1.13–1.17.

- [ ] `P2-01` Профили 1.13–1.17: DataVersion, Java, pack/datapack, dimensions, height.
- [ ] `P2-02` Chunk layout adapters 1.13–1.17.
- [ ] `P2-03` Biome adapters (column/section) + conversion.
- [ ] `P2-04` Blockstate/model/texture differences.
- [ ] `P2-05` Import/edit/export для 1.13–1.17.
- [ ] `P2-06` Generation: server flags/level-type/Java.
- [ ] `P2-07` Structures + version-aware `defaultOriginY`.
- [ ] `P2-08` Asset pipeline для старых pack format.
- [ ] `P2-09` Фикстуры + E2E + проверка каталога блоков/предметов для 1.13–1.17.
- [ ] `P2-10` Обновить матрицу/docs.
- [ ] `P2-11` Коммит P2.

### 21.5. P3 — legacy и форматы

**Цель:** 1.8–1.12 native/convert, pre-1.8 convert/passthrough, snapshots read-only, структуры и архивы.

- [ ] `P3-01` Профили 1.8–1.12 и pre-1.8.
- [ ] `P3-02` Numeric ID + metadata модель, item IDs и mapping в общий каталог.
- [ ] `P3-03` Codec 1.8–1.12 import/export.
- [ ] `P3-04` McRegion reader.
- [ ] `P3-05` Legacy import + passthrough.
- [ ] `P3-06` Convert через official server upgrade.
- [ ] `P3-07` Convert report: mapped/unknown/отказ от downgrade.
- [ ] `P3-08` `PassthroughCodec`: byte-for-byte + тесты.
- [ ] `P3-09` `.schem` v1–v3 import/export.
- [ ] `P3-10` `.schematic` import + limited export.
- [ ] `P3-11` `.litematic` read-only.
- [ ] `P3-12` Structure preview + entity/block-entity warnings.
- [ ] `P3-13` Round-trip verification структур.
- [ ] `P3-14` World `.zip` import/export + traversal/symlink/bomb protection.
- [ ] `P3-15` Project archive export/import.
- [ ] `P3-16` Snapshots: read-only/convert + advanced toggle.
- [ ] `P3-17` Фикстуры legacy + проверка numeric ID/metadata mapping в каталог + тесты.
- [ ] `P3-18` Обновить матрицу/docs + коммит P3.

### 21.6. P4 — UI/UX и сцена

**Цель:** production-ready оболочка и нормальная 3D-сцена.

#### A. Рабочее пространство

- [ ] `P4-01` Workspace: resizable panels + persisted layout.
- [ ] `P4-02` Правый `Toolbox` shell: вкладка рядом с Inspector, пустое состояние и навигация к инструментам.
- [ ] `P4-03` Перенести Terrain/Heightmap/Biomes/Generators/Templates/Objects/Parametric/DSL в Toolbox.
- [ ] `P4-04` Финальные 4 вкладки: Blocks / Layers / Analysis / Activity.
- [ ] `P4-05` Project Settings: General/Version/Components/World/Storage/Danger.
- [ ] `P4-06` UI upgrade версии + preflight + отчёт.
- [ ] `P4-07` Delete в Recycle Bin, relocate, duplicate, export archive.
- [ ] `P4-08` Rename: явно разделить display name и папку.
- [ ] `P4-09` Top bar: project/version/dirty, undo/redo/save, меню, window controls.
- [ ] `P4-10` HUD: dimension/coords, tool rail, View menu, help `?`, убрать постоянную подпись.
- [ ] `P4-11` WorldTree: dimensions из `WorldInfo`, честный `loaded`.

#### B. Окружение и свет

- [ ] `P4-12` `SceneEnvironment`: sky dome/gradient, sun/moon, stars, clouds, fog, dimension presets.
- [ ] `P4-13` Environment popover: время/slider/auto/weather/fog, persist per project.
- [ ] `P4-14` Lighting A: Lambert/custom material, face shading, sky/fog.
- [ ] `P4-15` Lighting B: sky/block light в DTO, vertex attributes, shader, AO, fallback.
- [ ] `P4-16` Lighting C: biome tint, emissive, weather particles, screenshot metadata.
- [ ] `P4-17` Adaptive grid 1/16/256 + chunk borders по minY/maxY + настройки.

#### C. Камера, производительность, качество

- [ ] `P4-18` `CameraRig`: fly/orbit/top-down, speed, smoothing, pointer lock.
- [ ] `P4-19` Focus `F`, frame `Home`, goto `Ctrl+G`, bookmarks, camera per project.
- [ ] `P4-20` Performance: lazy atlas, budget, culling/fog, dimension streaming, debug panel.
- [ ] `P4-21` Accessibility: focus-visible, keyboard tabs/menus, aria, reduced motion.
- [ ] `P4-22` Empty states + error presenter (причина/действие/Retry/logs).
- [ ] `P4-23` UI/scene тесты.
- [ ] `P4-24` Обновить docs по сцене.

#### D. UI-система по референсам

- [ ] `P4-25` UI-каркас на shadcn default: `components.json`, CSS-переменные темы, density через штатные size-варианты; без собственной палитры.
- [ ] `P4-26` Mode system `World / Build / Paint / Analyze` + свой tool rail для каждого режима.
- [ ] `P4-27` Inspector `Tool / Selection / World`; Environment в `World`; убрать отдельный environment-хаб.
- [ ] `P4-28` Toolbox: категории, поиск, pinned, capabilities, требования; только инструменты текущего режима/версии.
- [ ] `P4-29` Command palette + полная shortcut-карта; не перехватывать ввод в текстовых полях.
- [ ] `P4-30` Resize/collapse/persist панелей; минимальное окно 1280×800.
- [ ] `P4-31` Empty/error/disabled/read-only состояния; визуальный QA на соответствие штатному shadcn.
- [ ] `P4-32` Motion 120–180 ms + reduced motion; контраст AA, focus-visible, keyboard tabs.
- [ ] `P4-33` UI acceptance тесты: режимы, Inspector, Toolbox, top bar, нижние вкладки, layout persistence.
- [ ] `P4-34` Обновить UI-документацию (каркас, режимы, Inspector/Toolbox).

#### E. shadcn-first

- [ ] `P4-35` Аудит shadcn: `components.json`, installed components, registry; карта planned UI → available components.
- [ ] `P4-36` Использовать shadcn shell-примитивы (button, dialog, dropdown, tabs, popover, tooltip, resizable, command, scroll-area, separator, sidebar) в дефолтном виде; не редактировать `components/ui/*` вручную; зафиксировать gap-ы.
- [ ] `P4-37` Для gap-ов — custom на Radix/shadcn primitives, в том же визуальном языке, с a11y; без второго дизайна и без перекраски готовых компонентов.
- [ ] `P4-38` Тесты custom-компонентов + shadcn audit checklist в ручном QA/CI.
- [ ] `P4-39` Зафиксировать shadcn default-policy в `docs/ui.md` и закоммитить P4.

### 21.7. P5 — production hardening

- [ ] `P5-01` Полная матрица фикстур, каталогов блоков/предметов и round-trip по всем поддерживаемым версиям.
- [ ] `P5-02` Security review: загрузки, архивы, конвертация, allowlist, fuzz.
- [ ] `P5-03` Performance budget + benchmark на reference machine.
- [ ] `P5-04` Docs/ADRs/changelog/legal notices.
- [ ] `P5-05` Release checklist.
- [ ] `P5-06` Ручной Tauri QA: window controls, drag, maximize/restore, resize, UIScale.
- [ ] `P5-07` Known limitations + support matrix финально.
- [ ] `P5-08` Все проверки зелёные; полный прогон.
- [ ] `P5-09` Release candidate коммит; push только с разрешения.

### 21.8. Журнал выполнения

| Дата | Блок | Что сделано | Коммит | Проверки |
| --- | --- | --- | --- | --- |
| 2026-11-24 | Components шага создания | Новый шаг Components между Version и Source (для Import — после Version, для Vanilla — перед Source) собирает Java, client JAR и server JAR из одного facade: карточки Auto/Use existing/Manual, managed preview с источником/размером/checksum, прогресс с Cancel, ошибки через общий presenter и повторная проверка статуса; placeholder стал явным действием (`components_use_placeholder` + facade + TS API), проходит требование редактирования, но остаётся unverified и не заявляет JAR/каталог/checksum; после готового client JAR шаг показывает точный каталог блоков/предметов; состояние шага привязано к profile+mode и сбрасывается при их смене; wizard требует готовые assets (Java — для Vanilla, иначе остаётся предупреждением; server — только для Vanilla) до Next; docs обновлены | см. `git log` | 683 Rust tests, 346 TS tests (1 skipped), clippy, fmt, typecheck/lint/format, shadcn audit, browser QA |
| 2026-11-24 | Source Vanilla | Шаг Source Vanilla получает выбранную версию и отклоняет генерацию, если она не совпадает с проверенным server target; форма уже покрывает seed/preset/radius/center/structures/time/memory/view distance/CPU и валидированные локальные datapacks, EULA принимается для конкретного JAR и версии, а запуск требует отдельного per-run consent; `GenerationLimits` и `GenerationEstimate` стали backend-owned: `server_generation_estimate` показывает ожидаемое число чанков, консервативный размер мира, требуемое и свободное место на volume запуска, лимиты полей берутся из `vanilla_setup_check`; `server_start_generation` повторяет disk preflight через общий `ensure_space` до создания run directory; UI показывает estimate, блокирует запуск при нехватке диска и объясняет time limit; Rust-тест на estimate/лимиты и TS-тесты на estimate, disk blocker и version guard; docs обновлены | см. `git log` | 682 Rust tests, 337 TS tests, clippy, fmt, typecheck/lint/format, shadcn audit, browser QA |
| 2026-11-24 | Source Import | Шаг Source Import принимает папку мира или `.zip` архив: `world_source` безопасно распаковывает архив в уникальный staging (лимиты entries/размера, отказ traversal/absolute/symlink/special/duplicate/reserved names, `create_new`), находит единственный `level.dat` на глубине до двух папок, удаляет staging после preview и держит его только до конца byte-for-byte копирования; `JavaWorldAnalysis` получил `sourcePath`, `sourceKind` и archive-report, `world_import_preview` возвращает blockers вместо падения; `project_import_world` принимает `strategy` и отклоняет `convert`, пока нет проверенного конвертера; wizard получил переключатель folder/.zip, file picker с фильтром, карточку анализа с dimensions/entities/archive counts, явный disabled `Convert` и проверку, что анализ относится к текущему пути; Review повторяет source kind и `Import as-is`; Rust-тесты на извлечение/безопасность/лимиты, TS-тесты на archive flow; docs обновлены | см. `git log` | 681 Rust tests, 334 TS tests, clippy, fmt, typecheck/lint/format, browser QA |
| 2026-11-24 | Source Empty/Flat | Шаг Source получил версионные параметры: backend владеет `FlatDefaults` (пресет, лимиты, быстрые блоки и биомы) и валидирует слои, толщину, нижний Y и биом против выбранного `VersionProfile` до записи; `CreateRequest` заменён на `CreateOptions` с `profileId` и `FlatPreset`, `project_create`/TS API/`useProjectStore` пробрасывают их; wizard показывает Empty с высотой мира, для Flat даёт редактировать слои (block/thickness/порядок, до лимита), нижний Y и биом, повторяет слои/биом в Review и блокирует `Next` при выходе за overworld-диапазон; store-тесты на кастомные слои/высоту/биом, unknown block, air, границы и неверный профиль; UI собран из штатных shadcn `Button`/`Card`/`Field`/`Input`/`Select`/`Badge`; docs обновлены | см. `git log` | 668 Rust tests, 333 TS tests, clippy, fmt, typecheck/lint/format, browser QA |
| 2026-11-24 | Version шага создания | `versions_catalog` отдаёт официальный manifest через общий `DownloadClient` и кэш `<app-data>/version-manifest.json`, сопоставляя каждую release/snapshot запись с проверенным профилем сборки; wizard получил шаг Version между Basics и Source (для Import — после анализа мира, версия зафиксирована из `level.dat`), поиск и группировку по серии, метки latest/recommended, snapshot toggle, карточку с DataVersion, Java, высотой, layout, pack-форматами, каталогом, support badge и честными предупреждениями; версии без профиля видны, но блокируют `Next`; Review показывает выбранную версию; UI собран из штатных shadcn `Button`/`Card`/`Alert`/`Badge`/`Checkbox`/`Field`/`Input`/`ScrollArea`; docs обновлены | см. `git log` | 663 Rust tests, 330 TS tests, clippy, fmt, typecheck/lint/format, browser QA |
| 2026-11-24 | Basics шага создания | Backend владеет library root (`Documents/FuryBuilder`, fallback в app data) и суффиксом `.furybuilder`; wizard показывает папку проекта, позволяет переопределить имя папки, сам добавляет суффикс и до `Next` отклоняет path separators/reserved characters; имя и папка сохраняются при переходах, для каждого режима показаны требования; Review повторяет папку и location; импорт и Vanilla получают выбранную папку через `project_import_world`/`project_import_generated_world`; `create_with_folder` не перезаписывает существующую папку и возвращает conflict; тесты store и wizard | см. `git log` | Rust tests, TS tests, clippy, fmt, typecheck/lint/format, browser QA |
| 2026-11-23 | Project wizard shell | Новый полноэкранный `ProjectWizard` открывается вместо dialog в режиме «нет проекта»: шаги Basics → Source → Review (Vanilla использует шаги Basics → Source), навигация по уже открытым шагам, отдельная валидация каждого шага с понятной ошибкой, `Step X of Y` и progress bar; Basics даёт имя и режим, Source показывает Empty/Flat, разбор импортируемого мира или генерацию Vanilla, Review повторяет сводку; отправка показывает `preparing`, ошибка остаётся в мастере с `Retry`, значения не теряются; UI собран из штатных shadcn `Button`/`Card`/`Alert`/`Badge`/`Field`/`Input`/`Progress`/`Separator`/`ToggleGroup`, visual QA в браузере без console errors; тесты на шаги, прогресс, валидацию, создание, импорт, retry и отмену | см. `git log` | 660 Rust tests, TS tests, clippy, fmt, typecheck/lint/format, browser QA |
| 2026-11-23 | Lifecycle tests | Закреплены оба инварианта: без ready-проекта `Viewport` не создаёт renderer и не вызывает ни одну IPC-команду, `resume` без сессии ограничивается `project_status`, а editor-команды не пишут и не перезагружают viewport; после закрытия проверяются отмена job, очистка selection/clipboard/last operation, world summary/camera/metrics, workspace policy/error, history, а renderer не реагирует на bus-события после dispose | см. `git log` | 660 Rust tests, TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Recoverable autosave | Welcome явно показывает восстановление: recent-row с verified autosave получает badge `Recoverable`, пояснение и кнопку `Recover`, карточка предупреждает о количестве таких проектов, а Recover открывает восстановленную ревизию через единственный `open`-путь; редакторский `RecoveryBanner` предлагает `Save now`/`Discard recovered changes`/`Later`, а невалидный autosave честно открывает saved revision; тесты на badge, alert, Recover-действие, отсутствие ложного recovery и пустой список | см. `git log` | 660 Rust tests, TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Session teardown | Закрытие проекта сначала инвалидирует autosave и ждёт уже начатую запись, затем `project_close` отменяет backend jobs; после успеха очищаются job, selection, clipboard, world summary, camera/metrics, layers, scene, parametric и undo history; `WorldStore.reset()` возвращает полный дефолт, `resetTransient` чистит last operation; поздний результат autosave отбрасывается по generation, а тик autosave работает только в `ready`; renderer dispose и снятие bus/asset listeners происходят при размонтировании editor shell; тесты на ожидание autosave, полный сброс состояния, отсутствие autosave после close и dispose renderer | см. `git log` | 660 Rust tests, TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Project gate | Единый предикат `projectAllowsEditor(phase, session)` управляет монтированием editor shell и viewport: рендерер, asset listeners, `world_info`, `render_snapshot`, `render_chunks`, `renderer_ready` и world-записи не выполняются до готовности проекта; editor commands и `applyExternalWorldChange` повторно проверяют предикат, поэтому глобальные hotkeys не редактируют мир на Welcome; `resume` вызывается только из startup и явного retry; удалён недостижимый no-project overlay и мёртвые «Open a project» ветки; тесты на отсутствие world/render вызовов и на отказ всех write-команд без проекта | см. `git log` | 660 Rust tests, TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Welcome screen | App больше не монтирует editor shell без проекта: top bar и status bar остаются, контент переключается между Welcome и редактором; Welcome показывает бренд, шесть недавних проектов с версией/DataVersion/размером/временем/`Recoverable`, `New project`/`Open project`/`Import world`, desktop-offline объяснение, library error с retry, preparing progress без viewport и lifecycle failure с `Retry`/`Dismiss`; `New`/`Import` открывают ProjectManager на нужном tab/mode, `Open` использует folder picker; startup один раз проверяет сохранённую сессию, `Viewport` больше не вызывает `resume`; введён `PROJECT_SCHEMA_VERSION = 2` и честные schema-warnings вместо ложного «newer schema»; UI собран из штатных shadcn `Button`/`Card`/`Alert`/`Badge`/`Empty`/`ScrollArea`/`Separator`, визуальный QA в браузере без console errors | см. `git log` | 660 Rust tests, 313 TS tests, clippy, fmt, typecheck/lint/format, shadcn audit, browser QA |
| 2026-11-23 | Project lifecycle | `useProjectStore` стал единственным источником фаз `closed → preparing → ready → saving/error`: create/open/import/restore/close переводят состояние явно, save идёт через `saving` с сохранением открытого мира, ошибка несёт стабильный `code`/`message`/`hint` и остаётся retryable; `resume`/`refreshSession` выводят фазу из сессии, `clearFailure` возвращает в `ready`/`closed`; тесты на preparing во время open, retry после failed create, saving без размонтирования, failed save с открытым проектом и close/refresh | см. `git log` | 660 Rust tests, 300 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Manual/placeholder tests | Новые тесты facade и probe: symlink/junction в `.minecraft` root, version folder, server JAR и Java executable отклоняется до сохранения выбора; client JAR другой версии и неизвестный Java major отклоняются; 32-bit Java никогда не `usable`; placeholder переживает новый `ComponentManager` и остаётся `unverified` без id/path/hash/size; helper создаёт symlink на Unix и junction на Windows без admin-прав | см. `git log` | 660 Rust tests, 295 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Mock HTTP suite | Новые integration-тесты `crates/download-pipeline/tests/mock_http.rs` на реальном loopback TCP-сервере: success + atomic publish, 404 без retry, read timeout, relative redirect на том же host, отказ redirect на чужой host, hash mismatch без retry и с очисткой temp, wrong size до публикации, cooperative cancel с удалением partial-файла, disconnect, retry 503 → 200; `Allowlist::for_local_test` допускает plain HTTP только для loopback ephemeral port, production allowlists по-прежнему требуют HTTPS:443; redirect resolver сохраняет схему только в тестовом режиме | см. `git log` | 653 Rust tests, 295 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Components API | Tauri shell владеет одним `ComponentManager`: runtime/assets/server commands читают его через accessors, добавлены `components_status/resolve/inventory/*_release/install/cancel/relink/remove/verify`; install стримит `furybuilder-component-progress` (throttled 250 ms), мутации — `furybuilder-component-changed` + старые kind-events, ошибки — `furybuilder-component-error`; `ErrorCode` получил `offline`/`disk_space`; facade отдаёт previews и inventory; TS `components-api.ts` + типы + contract-тесты | см. `git log` | 641 Rust tests, 295 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Download pipeline | Новый crate `download-pipeline` заменил три копии сетевых helper-ов: точный host allowlist на каждом redirect hop, HTTPS-only и default port, явный `FURYBUILDER_PROXY`/`FURYBUILDER_HTTPS_PROXY` или стандартные proxy env, offline (`FURYBUILDER_OFFLINE`, `VersionCatalog::load_offline`), bounded retry для transport/408/425/429/5xx, disk preflight с запасом, потоковая проверка размера и SHA-1/SHA-256, same-directory temp + atomic replace через `MoveFileExW`/`rename`; manifest cache, Temurin, client JAR и server JAR переведены на общий клиент, `settings` persist тоже atomic; новые коды `offline`/`disk_space` и UI hints; тесты allowlist/redirect/offline/proxy/retry/disk/atomic/verification | см. `git log` | 637 Rust tests, 293 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | EULA consent | `server.json` schema 3 хранит явные EULA-согласия отдельно для каждой пары «точная версия + SHA-1 JAR»; загрузка проверяет официальный URL, timestamp, SHA-1 и отсутствие дублей; `SavedEula::matches` требует одновременно версию, JAR и официальный URL, а `discover` больше не считает согласие действующим для изменившегося или невалидного JAR; запуск дополнительно проверяет per-run consent и matching EULA до создания run-каталога; `.furybuilder-run.json` schema 2 фиксирует версию, SHA-1, время EULA и consent, импорт завершённого мира сверяет их с отчётом; UI явно называет версию и JAR; тесты на чужую версию/JAR/URL, изменённый файл, несколько согласий, отсутствие consent и невалидный manifest | см. `git log` | 613 Rust tests, 293 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Server exact version | `ServerTarget` (version/DataVersion/Java major) протянут через release/install/select/EULA/status; Mojang resolution идёт через общий `VersionCatalog` и exact version JSON, снапшоты и версии без server download отклоняются; `inspect_server_jar` отделяет структурные факты от проверки target, а SHA-1/размер проверяются при потоковой загрузке; кэш переехал на `components/servers/<version>/server.jar` с manifest schema 2, `server.json` schema 2 помнит отдельный выбор на версию; facade больше не блокирует non-baseline server target, desktop Settings и isolated generation остаются baseline; тесты на вторую версию, неверные version/DataVersion/Java, snapshot, отсутствующий server download, размер и независимые выборы | см. `git log` | 609 Rust tests, 293 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Assets placeholder | Placeholder стал явным выбором: asset settings schema 3 хранит `placeholder` отдельно от `source`, выбор placeholder очищает и блокирует pack stack, facade сообщает `placeholder`/`unverified` без id/hash/path и никогда не `ready`, ручной источник заменяет placeholder; Tauri `assets_use_placeholder`, Settings UI с предупреждением, viewport badge «Placeholder textures» и отказ от синтетического bundle без client JAR; catalog и render bundle требуют реальный источник | см. `git log` | 603 Rust tests, 293 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Assets manual | Ручной источник принимается в трёх формах: `.minecraft` root, `versions/<id>` и выбранный `client.jar`; facade relink сохраняет canonical path, статус резолвит точный JAR, версию, DataVersion, SHA-1 и размер; рендер-стек принимает `version_directory`; Tauri `assets_save_source` и Settings UI получили выбор version folder; корректный JAR без launcher metadata используется с предупреждением, объявленный checksum mismatch отклоняется; из asset settings удалён schema 0/1 migration | см. `git log` | 599 Rust tests, 292 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Client JAR catalog | `asset-source::AssetCatalogService` строит `CatalogSnapshot` из точного client JAR: version.json, blockstates, block/item models, item definitions/models, textures, `en_us`, прямые tags и atlas sources; block/item ids, properties, display names, icon textures и counts; `catalogHash`/`catalogId`, process-local cache и bounded warnings; Tauri `assets_catalog` + TS `loadAssetCatalog`; malformed files warn, JAR без blockstates отклоняется; `docs/catalog.md` | см. `git log` | 593 Rust tests, 291 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Managed client JAR | Preview и install берут exact client download из официального version JSON; потоковая загрузка проверяет announced SHA-1/размер, пишет в `<components/assets>/<version>/client.jar` + exact `client.json` + `.furybuilder-assets.json`, публикует atomic rename и удаляет staging при ошибке/cancel; уже установленный JAR проверяется по хэшу и перекачивается при повреждении; facade `install(assets)` сохраняет managed selection, status находит managed JAR без сохранённого указателя, `remove` удаляет только FuryBuilder-owned cache; `fetch_version_json_document` отдаёт exact version JSON | см. `git log` | 588 Rust tests, 291 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Java manual | Ручной `java.exe` проверяется запуском `-XshowSettings:properties -version`; major/arch/vendor сохраняются в отчёте, 32-bit/unreadable/старее target блокируются, новее target разрешается с warning; тот же policy применяется к facade relink и server generation, discovery считает и сортирует usable runtimes и больше не переписывает managed source на manual; Settings UI разрешает выбрать newer и показывает предупреждение; старый и новый Java покрыты тестами | см. `git log` | 578 Rust tests, 291 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Java download pipeline | Скачивание Temurin уже проверяет announced size и потоковый SHA-256, пишет один temp-архив, распаковывает с traversal/symlink/лимитами, пробует Java и публикует atomic rename; добавлены self-cleaning `StagedRuntime` (staging удаляется на любом error/panic пути), очистка stale `.staging-*`/`.download-*` старше 24 часов перед install и при явном Clear, тесты на failed commit и безопасную очистку partial-файлов; mock HTTP остаётся в P0-31 | см. `git log` | 577 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Java by major | Adoptium выбирается по требуемому Java major профиля (8/16/17/21/25), а не 21; managed cache переехал на `components/java/<major>/<release-id>`, manifest schema 2 хранит major, удаление и проверка идут по logical id; probe/select/saved/facade target-aware, Java 17 не блокируется глобальным baseline; Tauri/TS/UI передают major | см. `git log` | 574 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Component facade | Новый crate `component-manager` (модель вынесена в `component-model`): единый status/resolve/relink/remove/verify/install/cancel, target-aware проверки, explicit placeholder и запрет неизвестного hash/version; Java/server install получили cooperative cancel с очисткой partial-файлов; per-id remove managed runtime/server; assets download остаётся явно недоступен до P0-23 | см. `git log` | 572 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Component model | Новый crate `component-manager`: kind `java|assets|server`, mode `managed|manual|placeholder`, состояния `ready/missing/unverified/incompatible/error/not_required`, checksum DTO и validation; `project-store` использует общий `ComponentMode` | см. `git log` | 558 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Project version reporting | `WorldInfo`/project summary/MCP `ping` берут версию из проекта и профиля; `AppInfo` больше не заявляет глобальный Minecraft target; Settings и Project Manager не подставляют глобальную версию; MCP ping отдаёт `targetMinecraftVersion`, `targetDataVersion`, `profileId` открытого проекта | см. `git log` | 552 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Checkpoint metadata v2 | Metadata пишет `checkpointSchemaVersion: 2`, `profileId`, label, reason и полный `editorState`; список и открытие проверяют схему, профиль, размер и SHA-256; restore применяет editor state; v1 не читается | см. `git log` | 551 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Snapshot v3 only | Удалены v1/v2 reader и их тесты: `.fwl` принимается только в v3; demo-тест проверяет profile target и номер формата снимка; docs описывают exact-match snapshot policy | см. `git log` | 550 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Snapshot v3 | `.fwl` пишет `profileId`, `dataVersion`, layout и biome mode; decoder сверяет заголовок с built-in профилем и восстанавливает версию мира; демо пересобрано на v3 с обновлённым checksum | см. `git log` | 551 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Project schema v1 removal | `project.json` читается только в v2: любой другой номер схемы отклоняется без миграции, backup-файла и перезаписи; старые migration-тесты заменены тестами текущего контракта; docs описывают exact-match schema policy | см. `git log` | 549 Rust tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Project schema v2 | `project.json` хранит `target` (version/profile/DataVersion/Java/pack formats) и `components` (java/assets/server); target сверяется с профилем, placeholder не выдаёт себя за проверенный компонент; демо переведено на v2 | см. `git log` | 549 Rust tests, clippy, fmt |
| 2026-11-23 | Version foundation | CI-grep запрещает `1.21.4`/`4189` вне каталога, фикстур и тестов; UI и support matrix читают версию из проекта/профиля | см. `git log` | 543 Rust tests, 290 TS tests, clippy, fmt, typecheck/lint/format |
| 2026-11-23 | Version foundation | Удалены `TARGET_*`; мир, импорт, структуры, проект, ассеты, Java и server берут версию из profile/world | см. `git log` | 542 tests, clippy, fmt |
| 2026-11-23 | Version foundation | Инварианты профилей: capabilities, release/family, уникальные dimensions, full ⇒ write | см. `git log` | 43 tests, clippy, fmt |
| 2026-11-23 | Version foundation | Сборка профиля из family defaults + version JSON + `version.json` JAR; baseline переведён на один путь | см. `git log` | 42 tests, clippy, fmt |
| 2026-11-23 | Version foundation | Version JSON: Java major, client/server/asset downloads, SHA-1, allowlist | см. `git log` | 34 tests, clippy, fmt |
| 2026-11-23 | Version foundation | `VersionCatalog`: manifest fetch, TTL cache, offline fallback, host allowlist | см. `git log` | 27 tests, clippy, fmt |
| 2026-11-23 | Version foundation | Базовый профиль `java-classic-1.21.4`: 4189, Java 21, pack 46, datapack 61, modern dimensions, named catalog | см. `git log` | 20 tests, clippy, fmt |
| 2026-11-23 | Version foundation | `SupportLevel` + capability invariants: `full` требует write/export/assets, read-only/convert/out-of-scope не пишут | см. `git log` | 14 tests, clippy, fmt |
| 2026-11-23 | Version foundation | `VersionProfile`: layout, chunk/biome family, dimensions, Java, catalog, structures, server, capabilities; camelCase serde | см. `git log` | 11 tests, clippy, fmt |
| 2026-11-23 | Version foundation | `VersionId`, `VersionFamily`, `ReleaseType`: парсинг, numeric/stage ordering, serde | см. `git log` | 8 tests, clippy, fmt |
| 2026-11-23 | Version foundation | Добавлен crate `minecraft-version` в workspace, без Tauri/UI зависимостей | см. `git log` | cargo check/test, fmt |
| 2026-11-23 | План | Политика чистой перезаписи: без миграций, legacy-кода и compatibility fallback-ов; demo пересоздаётся | см. `git log` | docs only |
| 2026-11-23 | План | shadcn default-first: компоненты не перекрашиваются, тема только через CSS-переменные | `5614bb7` | docs only |
| 2026-11-23 | План | UI-референсы, режимы, контекстный Inspector, полный каталог блоков/предметов | `58d37a9` | docs only |
| 2026-11-22 | План | План + рабочий чеклист | см. `git log` | docs only |

> Новые строки добавлять сверху вниз по ходу работы. В колонке «Проверки» писать фактический результат, а не «ok»: например, `290 tests, typecheck/lint/format green`.

### 21.9. Критерий готовности чеклиста

Чеклист считается закрытым, когда:

- все пункты `P0`–`P5` отмечены `[x]` или явно помечены как отменённые с причиной;
- Definition of Done из §18 выполнен;
- журнал §21.8 содержит коммит и проверки для каждого блока;
- рабочее дерево чистое, все проверки зелёные, документация актуальна.

