//! Lightweight, dependency-free internationalization.
//!
//! Every user-facing string maps to a [`Msg`] key. [`t`] resolves a key for the
//! active [`Language`] to a `&'static str`. English is the default and the
//! fallback intent for every key. Because the match in [`t`] is exhaustive over
//! `Msg`, the compiler guarantees that no key is ever forgotten.

use serde::{Deserialize, Serialize};

/// UI language. Stored in the config file and selectable in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "zh")]
    Chinese,
}

impl Language {
    /// All languages, in display/cycle order.
    pub const ALL: [Language; 6] = [
        Language::English,
        Language::Italian,
        Language::Spanish,
        Language::French,
        Language::German,
        Language::Chinese,
    ];

    /// Native name shown in the language selector.
    pub fn display_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Italian => "Italiano",
            Language::Spanish => "Español",
            Language::French => "Français",
            Language::German => "Deutsch",
            Language::Chinese => "中文",
        }
    }

    /// The next language in [`Language::ALL`], wrapping around.
    pub fn next(self) -> Language {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The previous language in [`Language::ALL`], wrapping around.
    pub fn prev(self) -> Language {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// A translatable message key. Each maps to one string per [`Language`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    // ── Shared help / action words ───────────────────────────────────────────
    Navigate,
    MoveUp,
    Select,
    Proceed,
    Back,
    Toggle,
    Quit,
    Continue,
    Cancel,
    Confirm,
    NewConversion,
    Adjust,
    EditText,
    Save,
    SwitchPanel,
    SwitchMode,
    AllAudio,
    AllSubs,
    CopyTracks,
    ToOpus,
    AllOpus,
    AlreadyOpus,
    OpusUnavailable,
    AudioMode,
    OpusBitratePerChannel,
    SkipAlreadyOpus,
    OpenFolderAction,
    SelectThisFolder,
    SwitchFile,
    Cancelling,
    ShuttingDown,

    // ── Home ─────────────────────────────────────────────────────────────────
    MenuTitle,
    HomeOpenFile,
    HomeOpenFolder,
    HomeOpenFolderRecursive,
    HomeRipDisc,
    DiscSelectDrive,
    DiscSelectTitles,
    DiscNothingSelected,
    DiscOpenFolder,
    DiscSelectFolder,
    DiscScanThisFolder,
    DiscScanThisImage,
    DiscScanning,
    DiscDiscovering,
    DiscNoTitles,
    DiscRipAction,
    DiscChapters,
    WebAddDisc,
    StatusRipping,
    DiscNotInstalled,
    DiscNoDrive,
    DiscDriveEmpty,
    DiscPermissionDenied,
    DiscKeyExpired,
    DiscUnreadable,
    DiscInsufficientSpace,
    DiscChanged,
    DiscNoDestination,
    DiscNotADiscFolder,
    DiscFailedPrefix,
    Configuration,
    EncoderLabel,
    VmafDisabled,
    VmafEnabledOpen,
    DepsNotAvailable,

    // ── Explorer ─────────────────────────────────────────────────────────────
    CurrentDirectory,
    Notice,
    SelectVideoFile,
    SelectFolder,
    SelectedWord,
    NoVideoFiles,
    ScanningFiles,
    FolderScanFailed,

    // ── File confirm ─────────────────────────────────────────────────────────
    ConfirmSelection,
    Files,
    FilesSelectedWord,

    // ── Track config ─────────────────────────────────────────────────────────
    FileLabel,
    ResolutionLabel,
    TypeLabel,
    ModeLabel,
    OutputFileLabel,
    RemuxOnly,
    EncodeVideo,
    VideoInfo,
    AudioTracks,
    SubtitleTracks,
    SpaceToToggle,
    ForcedTag,
    Unknown,

    // ── Dolby Vision dialog ──────────────────────────────────────────────────
    DvDialogTitle,
    DvDialogPrompt,
    DvOptionKeep,
    DvOptionKeepDesc,
    DvOptionHdr10,
    DvOptionHdr10Desc,
    DvP5Warning,
    DvRecommended,
    DvRequiresSvt,
    DvModeHelp,
    DvKeptTag,
    UseRecommended,

    // ── Queue ────────────────────────────────────────────────────────────────
    AnalyzingFilesTitle,
    Encoding,
    ConversionQueue,
    Status,
    Waiting,
    Complete,
    StatusAnalyzing,
    StatusConfiguring,
    StatusReady,
    StatusVerifying,
    StatusDone,
    Elapsed,
    Eta,
    Cancelled,
    Error,

    // ── Finish ───────────────────────────────────────────────────────────────
    ConversionComplete,
    Success,
    QualityWarning,
    SourceLabel,
    OutputLabel,
    ReductionLabel,
    SizeIncrease,
    SourceFileDeleted,
    SourceKept,
    TimeLabel,
    ResultTitle,
    Summary,
    Results,
    Converted,
    Skipped,
    Errors,
    TotalSpaceSaved,
    TotalSpaceIncreased,
    TotalTime,
    ThresholdLabel,
    SourceDeletedTag,
    SourceKeptTag,

    // ── Quality descriptions ─────────────────────────────────────────────────
    QualExcellent,
    QualVeryGood,
    QualGood,
    QualFair,
    QualPoor,
    QualBad,

    // ── Confirm dialog ───────────────────────────────────────────────────────
    CancelEncodingTitle,
    CancelEncodingPrompt,
    CancelDiscTitle,
    CancelDiscPrompt,
    ExitAppTitle,
    ExitAppPrompt,
    ExitAppActivePrompt,
    AbandonTrackConfigTitle,
    AbandonTrackConfigPrompt,
    DiscardConfigTitle,
    DiscardConfigPrompt,
    CancelAnalysisTitle,
    CancelAnalysisPrompt,
    Yes,
    No,

    // ── Config screen ────────────────────────────────────────────────────────
    Settings,
    EnterToEdit,
    SavedExclaim,
    SaveFailed,
    UnsavedChanges,
    InvalidAddress,
    InvalidPort,
    CfgGroupTracks,
    CfgGroupDisc,
    CfgVmafThreshold,
    CfgVmafEnabled,
    CfgDeleteSource,
    CfgSvtPreset,
    CfgNvencPreset,
    CfgQualityPreset,
    QpLow,
    QpMedium,
    QpHigh,
    QpCustom,
    CfgRfSd,
    CfgRfHd,
    CfgRfFullHd,
    CfgRfFullHdHdr,
    CfgRfFullHdDv,
    CfgRfUhd,
    CfgRfUhdHdr,
    CfgRfUhdDv,
    CfgFilmGrain,
    CfgOutputSuffix,
    CfgOutputContainer,
    CfgSameDirectory,
    CfgAudioLanguages,
    CfgSubtitleLanguages,
    CfgLanguage,
    CfgDaemonEnabled,
    CfgDaemonAutostart,
    CfgDaemonAutostartHint,
    CfgDaemonBindAddress,
    CfgDaemonPort,
    CfgDaemonBrowseRoot,
    CfgMakemkvconPath,
    CfgStagingDirectory,
    CfgDaemonAuthToken,

    // ── Daemon mode ──────────────────────────────────────────────────────────
    DaemonDisabledError,
    DaemonListening,
    DaemonPublicHttp,
    DaemonTokenGenerated,
    EncoderUnavailable,
    DaemonShuttingDown,
    DaemonStarted,
    DaemonStartFailed,
    DaemonAlreadyRunning,
    DaemonRunning,
    DaemonNotRunning,
    DaemonStopped,
    DaemonStopFailed,
    DaemonStopHint,
    DaemonServiceInstalled,
    DaemonServiceUninstalled,
    DaemonServiceFailed,
    DaemonServiceUnsupported,
    DaemonServiceLingerHint,
    DaemonAutostartOn,
    DaemonAutostartOff,

    // ── Web UI ───────────────────────────────────────────────────────────────
    WebTabQueue,
    WebOffline,
    WebIdle,
    WebStatInQueue,
    WebCurrentFile,
    WebIdleNothing,
    WebOverallProgress,
    WebAddFile,
    WebAddFolder,
    WebAddFolderRecursive,
    WebClearFinished,
    WebSize,
    WebSaved,
    WebQueueEmpty,
    WebTagRemux,
    WebVmafFailed,
    WebLowVmaf,
    WebSelectFolderRecursive,
    WebHiddenFiles,
    WebParentDirectory,
    WebKindFolder,
    WebKindVideoFile,
    WebKindFile,
    WebKindSymlink,
    WebKindNotSelectable,
    WebTracksTitle,
    WebTracksHint,
    WebCloseTracks,
    WebTracksLocked,
    WebTracksUpdated,
    WebApplyRemaining,
    WebTracksApplied,
    WebNoAudioTracks,
    WebNoSubtitleTracks,
    WebSelectAll,
    WebClearAll,
    WebOptions,
    WebTrackExclude,
    WebAlreadyOpusCopied,
    WebRemuxHint,
    WebDolbyVision,
    WebDvRemuxHint,
    WebDvSourceHint,
    WebDvProfile,
    WebCancelling,
    WebRemovedFinished,
    WebAddedFiles,
    WebAlreadyQueued,
    WebNothingAdded,
    WebUnauthorized,
    WebRemoveFromQueue,
    WebGroupGeneral,
    WebGroupQuality,
    WebGroupPerformance,
    WebGroupOutput,
    WebGroupAudio,
    WebGroupDaemon,
    WebGroupDisc,
    WebGroupRateFactors,
    WebCfgOutputDirectory,
    WebCfgSelectAllFallback,
    WebCfgAudioDefault,
    WebCfgAudioModeCopy,
    WebCfgAudioModeOpus,
    WebSettingsNote,
    WebDaemonNote,
    WebLocalOnlyNote,
    WebRestartRequired,
    WebTokenHint,
    WebDismissSummary,
    WebDismiss,
    WebScanning,
    WebDeleteSourceWarning,
    WebSaveSettings,
    WebBackToQueue,
    WebConfirmTracks,
    WebSessionTotals,
    WebApplyRemainingHint,
    WebSaving,
    WebBrowse,
    WebLoading,
    WebFinishedWithErrors,
    WebConversionStopped,
    TerminalTooSmall,
}

/// Resolve a message key for the given language.
#[allow(clippy::too_many_lines)]
pub fn t(lang: Language, msg: Msg) -> &'static str {
    use Language::{
        Chinese as Zh, English as En, French as Fr, German as De, Italian as It, Spanish as Es,
    };
    match msg {
        // ── Shared help / action words ───────────────────────────────────────
        Msg::Navigate => match lang {
            En => "Navigate",
            It => "Naviga",
            Es => "Navegar",
            Fr => "Naviguer",
            De => "Navigieren",
            Zh => "导航",
        },
        Msg::MoveUp => match lang {
            En => "Move up",
            It => "Sposta su",
            Es => "Mover arriba",
            Fr => "Monter",
            De => "Nach oben",
            Zh => "上移",
        },
        Msg::Select => match lang {
            En => "Select",
            It => "Seleziona",
            Es => "Seleccionar",
            Fr => "Sélectionner",
            De => "Auswählen",
            Zh => "选择",
        },
        Msg::Proceed => match lang {
            En => "Proceed",
            It => "Procedi",
            Es => "Continuar",
            Fr => "Continuer",
            De => "Weiter",
            Zh => "继续",
        },
        Msg::Back => match lang {
            En => "Back",
            It => "Indietro",
            Es => "Atrás",
            Fr => "Retour",
            De => "Zurück",
            Zh => "返回",
        },
        Msg::Toggle => match lang {
            En => "Toggle",
            It => "Attiva/disattiva",
            Es => "Alternar",
            Fr => "Basculer",
            De => "Umschalten",
            Zh => "切换",
        },
        Msg::Quit => match lang {
            En => "Quit",
            It => "Esci",
            Es => "Salir",
            Fr => "Quitter",
            De => "Beenden",
            Zh => "退出",
        },
        Msg::Continue => match lang {
            En => "Continue",
            It => "Continua",
            Es => "Continuar",
            Fr => "Continuer",
            De => "Fortfahren",
            Zh => "继续",
        },
        Msg::Cancel => match lang {
            En => "Cancel",
            It => "Annulla",
            Es => "Cancelar",
            Fr => "Annuler",
            De => "Abbrechen",
            Zh => "取消",
        },
        Msg::Confirm => match lang {
            En => "Confirm",
            It => "Conferma",
            Es => "Confirmar",
            Fr => "Confirmer",
            De => "Bestätigen",
            Zh => "确认",
        },
        Msg::NewConversion => match lang {
            En => "New conversion",
            It => "Nuova conversione",
            Es => "Nueva conversión",
            Fr => "Nouvelle conversion",
            De => "Neue Konvertierung",
            Zh => "新建转换",
        },
        Msg::Adjust => match lang {
            En => "Adjust",
            It => "Regola",
            Es => "Ajustar",
            Fr => "Ajuster",
            De => "Anpassen",
            Zh => "调整",
        },
        Msg::EditText => match lang {
            En => "Edit text",
            It => "Modifica testo",
            Es => "Editar texto",
            Fr => "Modifier le texte",
            De => "Text bearbeiten",
            Zh => "编辑文本",
        },
        Msg::Save => match lang {
            En => "Save",
            It => "Salva",
            Es => "Guardar",
            Fr => "Enregistrer",
            De => "Speichern",
            Zh => "保存",
        },
        Msg::SwitchPanel => match lang {
            En => "Switch panel",
            It => "Cambia pannello",
            Es => "Cambiar panel",
            Fr => "Changer de panneau",
            De => "Bereich wechseln",
            Zh => "切换面板",
        },
        Msg::SwitchMode => match lang {
            En => "Switch mode",
            It => "Cambia modalità",
            Es => "Cambiar modo",
            Fr => "Changer de mode",
            De => "Modus wechseln",
            Zh => "切换模式",
        },
        Msg::AllAudio => match lang {
            En => "All audio",
            It => "Tutto l'audio",
            Es => "Todo el audio",
            Fr => "Tout l'audio",
            De => "Alle Audio",
            Zh => "全部音频",
        },
        Msg::AllSubs => match lang {
            En => "All subs",
            It => "Tutti i sottotitoli",
            Es => "Todos los subtítulos",
            Fr => "Tous les sous-titres",
            De => "Alle Untertitel",
            Zh => "全部字幕",
        },
        Msg::CopyTracks => match lang {
            En => "Copy",
            It => "Copia",
            Es => "Copiar",
            Fr => "Copier",
            De => "Kopieren",
            Zh => "复制",
        },
        Msg::ToOpus => match lang {
            En => "To Opus",
            It => "In Opus",
            Es => "A Opus",
            Fr => "En Opus",
            De => "Zu Opus",
            Zh => "转为 Opus",
        },
        Msg::AllOpus => match lang {
            En => "All to Opus",
            It => "Tutto in Opus",
            Es => "Todo a Opus",
            Fr => "Tout en Opus",
            De => "Alles zu Opus",
            Zh => "全部转为 Opus",
        },
        Msg::AlreadyOpus => match lang {
            En => "already Opus",
            It => "già Opus",
            Es => "ya es Opus",
            Fr => "déjà Opus",
            De => "bereits Opus",
            Zh => "已是 Opus",
        },
        Msg::OpusUnavailable => match lang {
            En => "libopus missing",
            It => "libopus assente",
            Es => "falta libopus",
            Fr => "libopus absent",
            De => "libopus fehlt",
            Zh => "缺少 libopus",
        },
        Msg::AudioMode => match lang {
            En => "Audio tracks",
            It => "Tracce audio",
            Es => "Pistas de audio",
            Fr => "Pistes audio",
            De => "Audiospuren",
            Zh => "音频轨道",
        },
        Msg::OpusBitratePerChannel => match lang {
            En => "Opus kbps per channel",
            It => "Opus kbps per canale",
            Es => "Opus kbps por canal",
            Fr => "Opus kbps par canal",
            De => "Opus kbps pro Kanal",
            Zh => "每声道 Opus kbps",
        },
        Msg::SkipAlreadyOpus => match lang {
            En => "Skip tracks already in Opus",
            It => "Salta tracce già in Opus",
            Es => "Omitir pistas ya en Opus",
            Fr => "Ignorer les pistes déjà en Opus",
            De => "Bereits in Opus vorliegende Spuren überspringen",
            Zh => "跳过已是 Opus 的轨道",
        },
        Msg::OpenFolderAction => match lang {
            En => "Open folder",
            It => "Apri cartella",
            Es => "Abrir carpeta",
            Fr => "Ouvrir le dossier",
            De => "Ordner öffnen",
            Zh => "打开文件夹",
        },
        Msg::SelectThisFolder => match lang {
            En => "Select this folder",
            It => "Seleziona questa cartella",
            Es => "Seleccionar esta carpeta",
            Fr => "Sélectionner ce dossier",
            De => "Diesen Ordner wählen",
            Zh => "选择此文件夹",
        },
        Msg::SwitchFile => match lang {
            En => "Switch file",
            It => "Cambia file",
            Es => "Cambiar archivo",
            Fr => "Changer de fichier",
            De => "Datei wechseln",
            Zh => "切换文件",
        },
        Msg::Cancelling => match lang {
            En => "Cancelling…",
            It => "Annullamento…",
            Es => "Cancelando…",
            Fr => "Annulation…",
            De => "Abbruch…",
            Zh => "正在取消…",
        },
        Msg::ShuttingDown => match lang {
            En => "Shutting down…",
            It => "Arresto in corso…",
            Es => "Cerrando…",
            Fr => "Arrêt en cours…",
            De => "Wird beendet…",
            Zh => "正在关闭…",
        },

        // ── Home ─────────────────────────────────────────────────────────────
        Msg::MenuTitle => match lang {
            En | It | Fr => "Menu",
            Es => "Menú",
            De => "Menü",
            Zh => "菜单",
        },
        Msg::HomeOpenFile => match lang {
            En => "Open video file",
            It => "Apri file video",
            Es => "Abrir archivo de video",
            Fr => "Ouvrir un fichier vidéo",
            De => "Videodatei öffnen",
            Zh => "打开视频文件",
        },
        Msg::HomeOpenFolder => match lang {
            En => "Open folder",
            It => "Apri cartella",
            Es => "Abrir carpeta",
            Fr => "Ouvrir un dossier",
            De => "Ordner öffnen",
            Zh => "打开文件夹",
        },
        Msg::HomeOpenFolderRecursive => match lang {
            En => "Open folder (recursive)",
            It => "Apri cartella (ricorsivo)",
            Es => "Abrir carpeta (recursivo)",
            Fr => "Ouvrir un dossier (récursif)",
            De => "Ordner öffnen (rekursiv)",
            Zh => "打开文件夹（递归）",
        },
        Msg::HomeRipDisc => match lang {
            En => "Rip DVD / Blu-ray",
            It => "Estrai da DVD / Blu-ray",
            Es => "Extraer de DVD / Blu-ray",
            Fr => "Extraire un DVD / Blu-ray",
            De => "DVD / Blu-ray auslesen",
            Zh => "抓取 DVD / 蓝光",
        },
        Msg::DiscSelectDrive => match lang {
            En => "Select Drive",
            It => "Seleziona unità",
            Es => "Seleccionar unidad",
            Fr => "Choisir le lecteur",
            De => "Laufwerk wählen",
            Zh => "选择光驱",
        },
        Msg::DiscNothingSelected => match lang {
            En => "Mark a title with Space before ripping.",
            It => "Contrassegna un titolo con Spazio prima di estrarre.",
            Es => "Marca un título con Espacio antes de extraer.",
            Fr => "Marquez un titre avec Espace avant d'extraire.",
            De => "Markieren Sie einen Titel mit Leertaste vor dem Rippen.",
            Zh => "开始提取前请用空格键标记标题。",
        },
        Msg::DiscSelectTitles => match lang {
            En => "Select Titles",
            It => "Seleziona titoli",
            Es => "Seleccionar títulos",
            Fr => "Choisir les titres",
            De => "Titel wählen",
            Zh => "选择标题",
        },
        Msg::DiscOpenFolder => match lang {
            En => "Open a disc folder…",
            It => "Apri una cartella disco…",
            Es => "Abrir una carpeta de disco…",
            Fr => "Ouvrir un dossier de disque…",
            De => "Disc-Ordner öffnen…",
            Zh => "打开光盘文件夹…",
        },
        Msg::DiscSelectFolder => match lang {
            En => "Select Disc Folder",
            It => "Seleziona cartella disco",
            Es => "Seleccionar carpeta de disco",
            Fr => "Choisir le dossier du disque",
            De => "Disc-Ordner wählen",
            Zh => "选择光盘文件夹",
        },
        Msg::DiscScanThisFolder => match lang {
            En => "Scan this disc",
            It => "Analizza questo disco",
            Es => "Analizar este disco",
            Fr => "Analyser ce disque",
            De => "Diese Disc lesen",
            Zh => "扫描此光盘",
        },
        Msg::DiscScanThisImage => match lang {
            En => "Scan disc image",
            It => "Analizza immagine disco",
            Es => "Analizar imagen de disco",
            Fr => "Analyser l’image disque",
            De => "Disc-Abbild lesen",
            Zh => "扫描光盘映像",
        },
        Msg::DiscScanning => match lang {
            En => "Scanning disc…",
            It => "Scansione del disco…",
            Es => "Analizando el disco…",
            Fr => "Analyse du disque…",
            De => "Disc wird gelesen…",
            Zh => "正在扫描光盘…",
        },
        Msg::DiscDiscovering => match lang {
            En => "Discovering optical drives…",
            It => "Ricerca delle unità ottiche…",
            Es => "Buscando unidades ópticas…",
            Fr => "Recherche des lecteurs optiques…",
            De => "Optische Laufwerke werden gesucht…",
            Zh => "正在查找光驱…",
        },
        Msg::DiscNoTitles => match lang {
            En => "No titles found on this disc",
            It => "Nessun titolo trovato su questo disco",
            Es => "No se encontraron títulos en este disco",
            Fr => "Aucun titre trouvé sur ce disque",
            De => "Keine Titel auf dieser Disc gefunden",
            Zh => "此光盘上未找到标题",
        },
        Msg::DiscRipAction => match lang {
            En => "Rip",
            It => "Estrai",
            Es => "Extraer",
            Fr => "Extraire",
            De => "Auslesen",
            Zh => "抓取",
        },
        Msg::WebAddDisc => match lang {
            En | De => "+ Disc",
            It | Es => "+ Disco",
            Fr => "+ Disque",
            Zh => "+ 光盘",
        },
        Msg::DiscChapters => match lang {
            En => "chapters",
            It => "capitoli",
            Es => "capítulos",
            Fr => "chapitres",
            De => "Kapitel",
            Zh => "章节",
        },
        Msg::StatusRipping => match lang {
            En => "Ripping",
            It => "Estrazione",
            Es => "Extrayendo",
            Fr => "Extraction",
            De => "Wird ausgelesen",
            Zh => "抓取中",
        },
        Msg::DiscNotInstalled => match lang {
            En => {
                "MakeMKV was not found. Install it from makemkv.com, or set makemkvcon_path under [disc] in config.toml."
            }
            It => {
                "MakeMKV non trovato. Installalo da makemkv.com oppure imposta makemkvcon_path in [disc] nel file config.toml."
            }
            Es => {
                "No se encontró MakeMKV. Instálalo desde makemkv.com o define makemkvcon_path en [disc] dentro de config.toml."
            }
            Fr => {
                "MakeMKV est introuvable. Installez-le depuis makemkv.com ou renseignez makemkvcon_path dans [disc] du fichier config.toml."
            }
            De => {
                "MakeMKV wurde nicht gefunden. Installieren Sie es von makemkv.com oder tragen Sie makemkvcon_path unter [disc] in config.toml ein."
            }
            Zh => {
                "未找到 MakeMKV。请从 makemkv.com 安装，或在 config.toml 的 [disc] 中设置 makemkvcon_path。"
            }
        },
        Msg::DiscNoDrive => match lang {
            En => "No optical drive was found",
            It => "Nessuna unità ottica trovata",
            Es => "No se encontró ninguna unidad óptica",
            Fr => "Aucun lecteur optique trouvé",
            De => "Kein optisches Laufwerk gefunden",
            Zh => "未找到光驱",
        },
        Msg::DiscDriveEmpty => match lang {
            En => "The drive is empty. Insert a disc and try again.",
            It => "L'unità è vuota. Inserisci un disco e riprova.",
            Es => "La unidad está vacía. Inserta un disco e inténtalo de nuevo.",
            Fr => "Le lecteur est vide. Insérez un disque et réessayez.",
            De => "Das Laufwerk ist leer. Legen Sie eine Disc ein und versuchen Sie es erneut.",
            Zh => "光驱是空的。请放入光盘后重试。",
        },
        Msg::DiscPermissionDenied => match lang {
            En => {
                "The drive could not be opened: permission denied. On Linux, add your user to the 'cdrom' group."
            }
            It => {
                "Impossibile aprire l'unità: permesso negato. Su Linux aggiungi il tuo utente al gruppo 'cdrom'."
            }
            Es => {
                "No se pudo abrir la unidad: permiso denegado. En Linux, añade tu usuario al grupo 'cdrom'."
            }
            Fr => {
                "Impossible d'ouvrir le lecteur : permission refusée. Sous Linux, ajoutez votre utilisateur au groupe « cdrom »."
            }
            De => {
                "Das Laufwerk konnte nicht geöffnet werden: Zugriff verweigert. Fügen Sie Ihren Benutzer unter Linux der Gruppe „cdrom“ hinzu."
            }
            Zh => "无法打开光驱：权限被拒绝。在 Linux 上，请将您的用户加入 cdrom 组。",
        },
        Msg::DiscKeyExpired => match lang {
            En => {
                "MakeMKV's Blu-ray key has expired. Refresh it in MakeMKV; this is not a fault of this tool. DVDs are unaffected."
            }
            It => {
                "La chiave Blu-ray di MakeMKV è scaduta. Aggiornala in MakeMKV: non è un difetto di questo programma. I DVD non sono interessati."
            }
            Es => {
                "La clave Blu-ray de MakeMKV ha caducado. Actualízala en MakeMKV; no es un fallo de esta herramienta. Los DVD no se ven afectados."
            }
            Fr => {
                "La clé Blu-ray de MakeMKV a expiré. Renouvelez-la dans MakeMKV ; ce n'est pas un défaut de cet outil. Les DVD ne sont pas concernés."
            }
            De => {
                "Der Blu-ray-Schlüssel von MakeMKV ist abgelaufen. Erneuern Sie ihn in MakeMKV; das ist kein Fehler dieses Programms. DVDs sind nicht betroffen."
            }
            Zh => {
                "MakeMKV 的蓝光密钥已过期。请在 MakeMKV 中更新，这不是本工具的问题。DVD 不受影响。"
            }
        },
        Msg::DiscUnreadable => match lang {
            En => {
                "The disc could not be read. It may be damaged, unsupported, or have been ejected."
            }
            It => {
                "Impossibile leggere il disco. Potrebbe essere danneggiato, non supportato o essere stato espulso."
            }
            Es => {
                "No se pudo leer el disco. Puede estar dañado, no ser compatible o haber sido expulsado."
            }
            Fr => {
                "Le disque n'a pas pu être lu. Il est peut-être endommagé, non pris en charge ou a été éjecté."
            }
            De => {
                "Die Disc konnte nicht gelesen werden. Sie ist möglicherweise beschädigt, nicht unterstützt oder wurde ausgeworfen."
            }
            Zh => "无法读取光盘。它可能已损坏、不受支持或已被弹出。",
        },
        Msg::DiscInsufficientSpace => match lang {
            En => "Not enough free space in the staging directory for this title",
            It => "Spazio insufficiente nella cartella di staging per questo titolo",
            Es => "No hay espacio suficiente en la carpeta temporal para este título",
            Fr => "Espace insuffisant dans le dossier temporaire pour ce titre",
            De => "Nicht genug freier Speicher im Zwischenordner für diesen Titel",
            Zh => "暂存目录中没有足够空间存放此标题",
        },
        Msg::DiscChanged => match lang {
            En => "The disc in the drive is not the one that was scanned. Scan it again.",
            It => "Il disco nell'unità non è quello analizzato. Ripeti la scansione.",
            Es => "El disco de la unidad no es el que se analizó. Vuelve a analizarlo.",
            Fr => {
                "Le disque dans le lecteur n'est pas celui qui a été analysé. Relancez l'analyse."
            }
            De => "Die Disc im Laufwerk ist nicht die zuvor gelesene. Lesen Sie sie erneut ein.",
            Zh => "光驱中的光盘不是之前扫描的那张。请重新扫描。",
        },
        Msg::DiscNoDestination => match lang {
            En => {
                "Set an output directory in Settings before ripping: the encode cannot be written to the staging directory."
            }
            It => {
                "Imposta una cartella di destinazione nelle impostazioni prima di estrarre: la codifica non può essere scritta nella cartella di staging."
            }
            Es => {
                "Define una carpeta de salida en los ajustes antes de extraer: la codificación no puede escribirse en la carpeta temporal."
            }
            Fr => {
                "Choisissez un dossier de sortie dans les réglages avant d'extraire : l'encodage ne peut pas être écrit dans le dossier temporaire."
            }
            De => {
                "Legen Sie vor dem Auslesen einen Ausgabeordner in den Einstellungen fest: Die Kodierung kann nicht in den Zwischenordner geschrieben werden."
            }
            Zh => "抓取前请在设置中指定输出目录：编码结果不能写入暂存目录。",
        },
        Msg::DiscNotADiscFolder => match lang {
            En => {
                "That is not a ripped disc: pick a folder holding VIDEO_TS or BDMV, or an ISO image."
            }
            It => {
                "Questo non è un disco copiato: scegli una cartella che contenga VIDEO_TS o BDMV, oppure un'immagine ISO."
            }
            Es => {
                "Eso no es un disco copiado: elige una carpeta que contenga VIDEO_TS o BDMV, o una imagen ISO."
            }
            Fr => {
                "Ce n'est pas un disque copié : choisissez un dossier contenant VIDEO_TS ou BDMV, ou une image ISO."
            }
            De => {
                "Das ist keine kopierte Disc: Wählen Sie einen Ordner mit VIDEO_TS oder BDMV oder ein ISO-Abbild."
            }
            Zh => "这不是已复制的光盘：请选择包含 VIDEO_TS 或 BDMV 的文件夹，或一个 ISO 映像。",
        },
        Msg::DiscFailedPrefix => match lang {
            En => "MakeMKV reported",
            It => "MakeMKV ha segnalato",
            Es => "MakeMKV informó",
            Fr => "MakeMKV a signalé",
            De => "MakeMKV meldet",
            Zh => "MakeMKV 报告",
        },
        Msg::Configuration => match lang {
            En | Fr => "Configuration",
            It => "Configurazione",
            Es => "Configuración",
            De => "Konfiguration",
            Zh => "配置",
        },
        Msg::EncoderLabel => match lang {
            En | It | De => "Encoder",
            Es => "Codificador",
            Fr => "Encodeur",
            Zh => "编码器",
        },
        Msg::VmafDisabled => match lang {
            En => "VMAF quality validation disabled",
            It => "Validazione qualità VMAF disattivata",
            Es => "Validación de calidad VMAF desactivada",
            Fr => "Validation de qualité VMAF désactivée",
            De => "VMAF-Qualitätsprüfung deaktiviert",
            Zh => "VMAF 质量验证已禁用",
        },
        Msg::VmafEnabledOpen => match lang {
            En => "VMAF quality validation enabled (threshold: ",
            It => "Validazione qualità VMAF attiva (soglia: ",
            Es => "Validación de calidad VMAF activada (umbral: ",
            Fr => "Validation de qualité VMAF activée (seuil : ",
            De => "VMAF-Qualitätsprüfung aktiv (Schwelle: ",
            Zh => "VMAF 质量验证已启用（阈值：",
        },
        Msg::DepsNotAvailable => match lang {
            En => "Required Dependencies not available",
            It => "Dipendenze richieste non disponibili",
            Es => "Dependencias requeridas no disponibles",
            Fr => "Dépendances requises non disponibles",
            De => "Erforderliche Abhängigkeiten nicht verfügbar",
            Zh => "所需依赖项不可用",
        },

        // ── Explorer ─────────────────────────────────────────────────────────
        Msg::CurrentDirectory => match lang {
            En => "Current Directory",
            It => "Cartella corrente",
            Es => "Directorio actual",
            Fr => "Répertoire actuel",
            De => "Aktuelles Verzeichnis",
            Zh => "当前目录",
        },
        Msg::Notice => match lang {
            En => "Notice",
            It => "Avviso",
            Es => "Aviso",
            Fr => "Avis",
            De => "Hinweis",
            Zh => "提示",
        },
        Msg::SelectVideoFile => match lang {
            En => "Select Video File",
            It => "Seleziona file video",
            Es => "Seleccionar archivo de video",
            Fr => "Sélectionner un fichier vidéo",
            De => "Videodatei wählen",
            Zh => "选择视频文件",
        },
        Msg::SelectFolder => match lang {
            En => "Select Folder",
            It => "Seleziona cartella",
            Es => "Seleccionar carpeta",
            Fr => "Sélectionner un dossier",
            De => "Ordner wählen",
            Zh => "选择文件夹",
        },
        Msg::SelectedWord => match lang {
            En => "selected",
            It => "selezionati",
            Es => "seleccionados",
            Fr => "sélectionnés",
            De => "ausgewählt",
            Zh => "已选择",
        },
        Msg::NoVideoFiles => match lang {
            En => "No video files found in this folder",
            It => "Nessun file video trovato in questa cartella",
            Es => "No se encontraron archivos de video en esta carpeta",
            Fr => "Aucun fichier vidéo trouvé dans ce dossier",
            De => "Keine Videodateien in diesem Ordner gefunden",
            Zh => "此文件夹中未找到视频文件",
        },
        Msg::ScanningFiles => match lang {
            En => "Scanning files…",
            It => "Scansione dei file…",
            Es => "Escaneando archivos…",
            Fr => "Analyse des fichiers…",
            De => "Dateien werden durchsucht…",
            Zh => "正在扫描文件…",
        },
        Msg::FolderScanFailed => match lang {
            En => "Folder scan stopped unexpectedly",
            It => "La scansione della cartella si è interrotta inaspettatamente",
            Es => "El escaneo de la carpeta se detuvo inesperadamente",
            Fr => "L'analyse du dossier s'est arrêtée de façon inattendue",
            De => "Die Ordnersuche wurde unerwartet beendet",
            Zh => "文件夹扫描意外停止",
        },

        // ── File confirm ─────────────────────────────────────────────────────
        Msg::ConfirmSelection => match lang {
            En => "Confirm Selection",
            It => "Conferma selezione",
            Es => "Confirmar selección",
            Fr => "Confirmer la sélection",
            De => "Auswahl bestätigen",
            Zh => "确认选择",
        },
        Msg::Files => match lang {
            En => "Files",
            It => "File",
            Es => "Archivos",
            Fr => "Fichiers",
            De => "Dateien",
            Zh => "文件",
        },
        Msg::FilesSelectedWord => match lang {
            En => "files selected",
            It => "file selezionati",
            Es => "archivos seleccionados",
            Fr => "fichiers sélectionnés",
            De => "Dateien ausgewählt",
            Zh => "个文件已选择",
        },

        // ── Track config ─────────────────────────────────────────────────────
        Msg::FileLabel => match lang {
            En | It => "File",
            Es => "Archivo",
            Fr => "Fichier",
            De => "Datei",
            Zh => "文件",
        },
        Msg::ResolutionLabel => match lang {
            En => "Resolution",
            It => "Risoluzione",
            Es => "Resolución",
            Fr => "Résolution",
            De => "Auflösung",
            Zh => "分辨率",
        },
        Msg::TypeLabel => match lang {
            En | Fr => "Type",
            It | Es => "Tipo",
            De => "Typ",
            Zh => "类型",
        },
        Msg::ModeLabel => match lang {
            En | Fr => "Mode",
            It => "Modalità",
            Es => "Modo",
            De => "Modus",
            Zh => "模式",
        },
        Msg::OutputFileLabel => match lang {
            En => "Output File",
            It => "File di output",
            Es => "Archivo de salida",
            Fr => "Fichier de sortie",
            De => "Ausgabedatei",
            Zh => "输出文件",
        },
        Msg::RemuxOnly => match lang {
            En => "Remux Only (Copy Video)",
            It => "Solo remux (copia video)",
            Es => "Solo remux (copiar video)",
            Fr => "Remux seul (copier la vidéo)",
            De => "Nur Remux (Video kopieren)",
            Zh => "仅重封装（复制视频）",
        },
        Msg::EncodeVideo => match lang {
            En => "Encode Video (AV1)",
            It => "Codifica video (AV1)",
            Es => "Codificar video (AV1)",
            Fr => "Encoder la vidéo (AV1)",
            De => "Video kodieren (AV1)",
            Zh => "编码视频（AV1）",
        },
        // ── Dolby Vision dialog ──────────────────────────────────────────────
        Msg::DvDialogTitle => match lang {
            En => "Dolby Vision Source",
            It => "Sorgente Dolby Vision",
            Es => "Fuente Dolby Vision",
            Fr => "Source Dolby Vision",
            De => "Dolby-Vision-Quelle",
            Zh => "杜比视界源",
        },
        Msg::DvDialogPrompt => match lang {
            En => "This file contains Dolby Vision. How should it be converted?",
            It => "Questo file contiene Dolby Vision. Come deve essere convertito?",
            Es => "Este archivo contiene Dolby Vision. ¿Cómo debe convertirse?",
            Fr => "Ce fichier contient du Dolby Vision. Comment le convertir ?",
            De => "Diese Datei enthält Dolby Vision. Wie soll sie konvertiert werden?",
            Zh => "此文件包含杜比视界。要如何转换？",
        },
        Msg::DvOptionKeep => match lang {
            En => "AV1 with Dolby Vision (profile 10)",
            It => "AV1 con Dolby Vision (profilo 10)",
            Es => "AV1 con Dolby Vision (perfil 10)",
            Fr => "AV1 avec Dolby Vision (profil 10)",
            De => "AV1 mit Dolby Vision (Profil 10)",
            Zh => "AV1 保留杜比视界（Profile 10）",
        },
        Msg::DvOptionKeepDesc => match lang {
            En => "Keeps the dynamic metadata in the AV1 stream",
            It => "Mantiene i metadati dinamici nel flusso AV1",
            Es => "Mantiene los metadatos dinámicos en el flujo AV1",
            Fr => "Conserve les métadonnées dynamiques dans le flux AV1",
            De => "Behält die dynamischen Metadaten im AV1-Stream",
            Zh => "在 AV1 流中保留动态元数据",
        },
        Msg::DvOptionHdr10 => match lang {
            En => "AV1 with true HDR10",
            It => "AV1 con vero HDR10",
            Es => "AV1 con HDR10 verdadero",
            Fr => "AV1 avec HDR10 véritable",
            De => "AV1 mit echtem HDR10",
            Zh => "AV1 转为真正的 HDR10",
        },
        Msg::DvOptionHdr10Desc => match lang {
            En => "Drops the DV layer, keeps HDR10 static metadata",
            It => "Rimuove il livello DV, mantiene i metadati statici HDR10",
            Es => "Elimina la capa DV, mantiene los metadatos estáticos HDR10",
            Fr => "Supprime la couche DV, conserve les métadonnées statiques HDR10",
            De => "Entfernt die DV-Ebene, behält statische HDR10-Metadaten",
            Zh => "移除 DV 层，保留 HDR10 静态元数据",
        },
        Msg::DvP5Warning => match lang {
            En => {
                "Profile 5 has no HDR10 base layer: keeping DV needs a DV-capable player; HDR10 tone-maps on the GPU (Vulkan)."
            }
            It => {
                "Il profilo 5 non ha un livello base HDR10: mantenere DV richiede un lettore compatibile DV; HDR10 usa tone mapping su GPU (Vulkan)."
            }
            Es => {
                "El perfil 5 no tiene capa base HDR10: mantener DV requiere un reproductor compatible con DV; HDR10 usa tone mapping en GPU (Vulkan)."
            }
            Fr => {
                "Le profil 5 n'a pas de couche de base HDR10 : garder le DV exige un lecteur compatible DV ; HDR10 applique un tone mapping GPU (Vulkan)."
            }
            De => {
                "Profil 5 hat keine HDR10-Basisebene: DV behalten erfordert einen DV-fähigen Player; HDR10 nutzt GPU-Tone-Mapping (Vulkan)."
            }
            Zh => {
                "Profile 5 没有 HDR10 基础层：保留 DV 需要支持 DV 的播放器；转 HDR10 将使用 GPU 色调映射（Vulkan）。"
            }
        },
        Msg::DvRecommended => match lang {
            En => "recommended",
            It => "consigliato",
            Es => "recomendado",
            Fr => "recommandé",
            De => "empfohlen",
            Zh => "推荐",
        },
        Msg::DvRequiresSvt => match lang {
            En => "Dolby Vision passthrough requires the SVT-AV1 encoder",
            It => "Il passthrough Dolby Vision richiede l'encoder SVT-AV1",
            Es => "El passthrough de Dolby Vision requiere el codificador SVT-AV1",
            Fr => "Le passthrough Dolby Vision nécessite l'encodeur SVT-AV1",
            De => "Dolby-Vision-Passthrough erfordert den SVT-AV1-Encoder",
            Zh => "杜比视界直通需要 SVT-AV1 编码器",
        },
        Msg::DvModeHelp => match lang {
            En => "DV Mode",
            It | Es => "Modo DV",
            Fr => "Mode DV",
            De => "DV-Modus",
            Zh => "DV 模式",
        },
        Msg::DvKeptTag => match lang {
            En => "kept",
            It => "mantenuto",
            Es => "mantenido",
            Fr => "conservé",
            De => "beibehalten",
            Zh => "保留",
        },
        Msg::UseRecommended => match lang {
            En => "Use recommended",
            It => "Usa l’opzione consigliata",
            Es => "Usar la opción recomendada",
            Fr => "Utiliser la recommandation",
            De => "Empfehlung verwenden",
            Zh => "使用推荐设置",
        },

        Msg::VideoInfo => match lang {
            En => "Video Info",
            It => "Info video",
            Es => "Información del video",
            Fr => "Infos vidéo",
            De => "Video-Info",
            Zh => "视频信息",
        },
        Msg::AudioTracks => match lang {
            En => "Audio Tracks",
            It => "Tracce audio",
            Es => "Pistas de audio",
            Fr => "Pistes audio",
            De => "Audiospuren",
            Zh => "音频轨道",
        },
        Msg::SubtitleTracks => match lang {
            En => "Subtitle Tracks",
            It => "Tracce sottotitoli",
            Es => "Pistas de subtítulos",
            Fr => "Pistes de sous-titres",
            De => "Untertitelspuren",
            Zh => "字幕轨道",
        },
        Msg::SpaceToToggle => match lang {
            En => "Space to toggle",
            It => "Spazio per attivare",
            Es => "Espacio para alternar",
            Fr => "Espace pour basculer",
            De => "Leertaste zum Umschalten",
            Zh => "空格切换",
        },
        Msg::ForcedTag => match lang {
            En => "Forced",
            It => "Forzato",
            Es => "Forzado",
            Fr => "Forcé",
            De => "Erzwungen",
            Zh => "强制",
        },
        Msg::Unknown => match lang {
            En => "Unknown",
            It => "Sconosciuto",
            Es => "Desconocido",
            Fr => "Inconnu",
            De => "Unbekannt",
            Zh => "未知",
        },

        // ── Queue ────────────────────────────────────────────────────────────
        Msg::AnalyzingFilesTitle => match lang {
            En => "Analyzing Files...",
            It => "Analisi file...",
            Es => "Analizando archivos...",
            Fr => "Analyse des fichiers...",
            De => "Dateien werden analysiert...",
            Zh => "正在分析文件...",
        },
        Msg::Encoding => match lang {
            En => "Encoding",
            It => "Codifica",
            Es => "Codificando",
            Fr => "Encodage",
            De => "Kodierung",
            Zh => "编码中",
        },
        Msg::ConversionQueue => match lang {
            En => "Conversion Queue",
            It => "Coda di conversione",
            Es => "Cola de conversión",
            Fr => "File de conversion",
            De => "Konvertierungswarteschlange",
            Zh => "转换队列",
        },
        Msg::Status => match lang {
            En | De => "Status",
            It => "Stato",
            Es => "Estado",
            Fr => "État",
            Zh => "状态",
        },
        Msg::Waiting => match lang {
            En => "Waiting...",
            It => "In attesa...",
            Es => "Esperando...",
            Fr => "En attente...",
            De => "Warten...",
            Zh => "等待中...",
        },
        Msg::Complete => match lang {
            En => "Complete!",
            It => "Completato!",
            Es => "¡Completado!",
            Fr => "Terminé !",
            De => "Fertig!",
            Zh => "完成！",
        },
        Msg::StatusAnalyzing => match lang {
            En => "Analyzing...",
            It => "Analisi...",
            Es => "Analizando...",
            Fr => "Analyse...",
            De => "Analysiere...",
            Zh => "分析中...",
        },
        Msg::StatusConfiguring => match lang {
            En => "Configuring...",
            It => "Configurazione...",
            Es => "Configurando...",
            Fr => "Configuration...",
            De => "Konfiguriere...",
            Zh => "配置中...",
        },
        Msg::StatusReady => match lang {
            En => "Ready",
            It => "Pronto",
            Es => "Listo",
            Fr => "Prêt",
            De => "Bereit",
            Zh => "就绪",
        },
        Msg::StatusVerifying => match lang {
            En => "Verifying quality...",
            It => "Verifica qualità...",
            Es => "Verificando calidad...",
            Fr => "Vérification de la qualité...",
            De => "Qualität wird geprüft...",
            Zh => "正在验证质量...",
        },
        Msg::StatusDone => match lang {
            En => "Done",
            It => "Fatto",
            Es => "Hecho",
            Fr => "Terminé",
            De => "Fertig",
            Zh => "完成",
        },
        Msg::Elapsed => match lang {
            En => "Elapsed",
            It => "Trascorso",
            Es => "Transcurrido",
            Fr => "Écoulé",
            De => "Verstrichen",
            Zh => "已用时",
        },
        Msg::Eta => match lang {
            En | Es | Fr => "ETA",
            It => "Stima",
            De => "Restzeit",
            Zh => "预计剩余",
        },
        Msg::Cancelled => match lang {
            En => "Cancelled",
            It => "Annullato",
            Es => "Cancelado",
            Fr => "Annulé",
            De => "Abgebrochen",
            Zh => "已取消",
        },
        Msg::Error => match lang {
            En | Es => "Error",
            It => "Errore",
            Fr => "Erreur",
            De => "Fehler",
            Zh => "错误",
        },

        // ── Finish ───────────────────────────────────────────────────────────
        Msg::ConversionComplete => match lang {
            En => "Conversion Complete!",
            It => "Conversione completata!",
            Es => "¡Conversión completada!",
            Fr => "Conversion terminée !",
            De => "Konvertierung abgeschlossen!",
            Zh => "转换完成！",
        },
        Msg::Success => match lang {
            En => "Success",
            It => "Successo",
            Es => "Éxito",
            Fr => "Réussi",
            De => "Erfolg",
            Zh => "成功",
        },
        Msg::QualityWarning => match lang {
            En => "Quality Warning",
            It => "Avviso qualità",
            Es => "Advertencia de calidad",
            Fr => "Avertissement de qualité",
            De => "Qualitätswarnung",
            Zh => "质量警告",
        },
        Msg::SourceLabel => match lang {
            En | Fr => "Source",
            It => "Origine",
            Es => "Origen",
            De => "Quelle",
            Zh => "源文件",
        },
        Msg::OutputLabel => match lang {
            En | It => "Output",
            Es => "Salida",
            Fr => "Sortie",
            De => "Ausgabe",
            Zh => "输出",
        },
        Msg::ReductionLabel => match lang {
            En => "Reduction",
            It => "Riduzione",
            Es => "Reducción",
            Fr => "Réduction",
            De => "Reduzierung",
            Zh => "减少",
        },
        Msg::SizeIncrease => match lang {
            En => "Size increase",
            It => "Aumento dimensioni",
            Es => "Aumento de tamaño",
            Fr => "Augmentation de taille",
            De => "Größenzunahme",
            Zh => "大小增加",
        },
        Msg::SourceFileDeleted => match lang {
            En => "Source file deleted",
            It => "File di origine eliminato",
            Es => "Archivo de origen eliminado",
            Fr => "Fichier source supprimé",
            De => "Quelldatei gelöscht",
            Zh => "已删除源文件",
        },
        Msg::SourceKept => match lang {
            En => "Source kept",
            It => "Origine mantenuta",
            Es => "Origen conservado",
            Fr => "Source conservée",
            De => "Quelle behalten",
            Zh => "已保留源文件",
        },
        Msg::TimeLabel => match lang {
            En => "Time",
            It => "Tempo",
            Es => "Tiempo",
            Fr => "Temps",
            De => "Zeit",
            Zh => "用时",
        },
        Msg::ResultTitle => match lang {
            En => "Result",
            It => "Risultato",
            Es => "Resultado",
            Fr => "Résultat",
            De => "Ergebnis",
            Zh => "结果",
        },
        Msg::Summary => match lang {
            En => "Summary",
            It => "Riepilogo",
            Es => "Resumen",
            Fr => "Résumé",
            De => "Zusammenfassung",
            Zh => "摘要",
        },
        Msg::Results => match lang {
            En => "Results",
            It => "Risultati",
            Es => "Resultados",
            Fr => "Résultats",
            De => "Ergebnisse",
            Zh => "结果",
        },
        Msg::Converted => match lang {
            En => "Converted",
            It => "Convertiti",
            Es => "Convertidos",
            Fr => "Convertis",
            De => "Konvertiert",
            Zh => "已转换",
        },
        Msg::Skipped => match lang {
            En => "Skipped",
            It => "Saltati",
            Es => "Omitidos",
            Fr => "Ignorés",
            De => "Übersprungen",
            Zh => "已跳过",
        },
        Msg::Errors => match lang {
            En => "Errors",
            It => "Errori",
            Es => "Errores",
            Fr => "Erreurs",
            De => "Fehler",
            Zh => "错误",
        },
        Msg::TotalSpaceSaved => match lang {
            En => "Total space saved",
            It => "Spazio totale risparmiato",
            Es => "Espacio total ahorrado",
            Fr => "Espace total économisé",
            De => "Insgesamt gespart",
            Zh => "总共节省空间",
        },
        Msg::TotalSpaceIncreased => match lang {
            En => "Total size increase",
            It => "Aumento totale delle dimensioni",
            Es => "Aumento total de tamaño",
            Fr => "Augmentation totale de taille",
            De => "Gesamte Größenzunahme",
            Zh => "总大小增加",
        },
        Msg::TotalTime => match lang {
            En => "Total time",
            It => "Tempo totale",
            Es => "Tiempo total",
            Fr => "Temps total",
            De => "Gesamtzeit",
            Zh => "总用时",
        },
        Msg::ThresholdLabel => match lang {
            En => "threshold",
            It => "soglia",
            Es => "umbral",
            Fr => "seuil",
            De => "Schwelle",
            Zh => "阈值",
        },
        Msg::SourceDeletedTag => match lang {
            En => "source deleted",
            It => "origine eliminata",
            Es => "origen eliminado",
            Fr => "source supprimée",
            De => "Quelle gelöscht",
            Zh => "源文件已删除",
        },
        Msg::SourceKeptTag => match lang {
            En => "source kept",
            It => "origine mantenuta",
            Es => "origen conservado",
            Fr => "source conservée",
            De => "Quelle behalten",
            Zh => "源文件已保留",
        },

        // ── Quality descriptions ─────────────────────────────────────────────
        Msg::QualExcellent => match lang {
            En | Fr => "Excellent",
            It => "Eccellente",
            Es => "Excelente",
            De => "Ausgezeichnet",
            Zh => "优秀",
        },
        Msg::QualVeryGood => match lang {
            En => "Very Good",
            It => "Molto buono",
            Es => "Muy bueno",
            Fr => "Très bon",
            De => "Sehr gut",
            Zh => "很好",
        },
        Msg::QualGood => match lang {
            En => "Good",
            It => "Buono",
            Es => "Bueno",
            Fr => "Bon",
            De => "Gut",
            Zh => "良好",
        },
        Msg::QualFair => match lang {
            En => "Fair",
            It => "Discreto",
            Es => "Aceptable",
            Fr => "Correct",
            De => "Ausreichend",
            Zh => "一般",
        },
        Msg::QualPoor => match lang {
            En => "Poor",
            It => "Scarso",
            Es => "Pobre",
            Fr => "Médiocre",
            De => "Mangelhaft",
            Zh => "较差",
        },
        Msg::QualBad => match lang {
            En => "Bad",
            It => "Pessimo",
            Es => "Malo",
            Fr => "Mauvais",
            De => "Schlecht",
            Zh => "差",
        },

        // ── Confirm dialog ───────────────────────────────────────────────────
        Msg::CancelEncodingTitle => match lang {
            En => "Cancel Encoding",
            It => "Annulla codifica",
            Es => "Cancelar codificación",
            Fr => "Annuler l'encodage",
            De => "Kodierung abbrechen",
            Zh => "取消编码",
        },
        Msg::CancelEncodingPrompt => match lang {
            En => "Are you sure you want to cancel the current encoding?",
            It => "Vuoi davvero annullare la codifica in corso?",
            Es => "¿Seguro que quieres cancelar la codificación actual?",
            Fr => "Voulez-vous vraiment annuler l'encodage en cours ?",
            De => "Möchten Sie die laufende Kodierung wirklich abbrechen?",
            Zh => "确定要取消当前编码吗？",
        },
        Msg::CancelDiscTitle => match lang {
            En => "Cancel Disc Operation",
            It => "Annulla operazione disco",
            Es => "Cancelar operación de disco",
            Fr => "Annuler l'opération disque",
            De => "Disc-Vorgang abbrechen",
            Zh => "取消光盘操作",
        },
        Msg::CancelDiscPrompt => match lang {
            En => "Cancel the current disc scan or rip?",
            It => "Annullare la scansione o l'estrazione del disco in corso?",
            Es => "¿Cancelar el escaneo o la extracción del disco actual?",
            Fr => "Annuler l'analyse ou l'extraction du disque en cours ?",
            De => "Den laufenden Disc-Scan oder Rip abbrechen?",
            Zh => "取消当前的光盘扫描或提取吗？",
        },
        Msg::ExitAppTitle => match lang {
            En => "Exit Application",
            It => "Esci dall'applicazione",
            Es => "Salir de la aplicación",
            Fr => "Quitter l'application",
            De => "Anwendung beenden",
            Zh => "退出应用程序",
        },
        Msg::ExitAppPrompt => match lang {
            En => "Are you sure you want to exit?",
            It => "Vuoi davvero uscire?",
            Es => "¿Seguro que quieres salir?",
            Fr => "Voulez-vous vraiment quitter ?",
            De => "Möchten Sie wirklich beenden?",
            Zh => "确定要退出吗？",
        },
        Msg::ExitAppActivePrompt => match lang {
            En => "Active work will be cancelled before exit. Continue?",
            It => "Il lavoro attivo verrà annullato prima dell'uscita. Continuare?",
            Es => "El trabajo activo se cancelará antes de salir. ¿Continuar?",
            Fr => "Le travail en cours sera annulé avant de quitter. Continuer ?",
            De => "Aktive Vorgänge werden vor dem Beenden abgebrochen. Fortfahren?",
            Zh => "退出前将取消正在进行的任务。是否继续？",
        },
        Msg::AbandonTrackConfigTitle => match lang {
            En => "Discard Batch",
            It => "Annulla lotto",
            Es => "Descartar lote",
            Fr => "Abandonner le lot",
            De => "Stapel verwerfen",
            Zh => "放弃批次",
        },
        Msg::AbandonTrackConfigPrompt => match lang {
            En => "Are you sure you want to discard this batch and return home?",
            It => "Vuoi davvero annullare questo lotto e tornare al menu principale?",
            Es => "¿Seguro que quieres descartar este lote y volver al inicio?",
            Fr => "Voulez-vous vraiment abandonner ce lot et revenir à l'accueil ?",
            De => "Möchten Sie diesen Stapel wirklich verwerfen und zum Hauptmenü zurückkehren?",
            Zh => "确定要放弃此批次并返回主页吗？",
        },
        Msg::DiscardConfigTitle => match lang {
            En => "Discard Changes",
            It => "Annulla modifiche",
            Es => "Descartar cambios",
            Fr => "Annuler les modifications",
            De => "Änderungen verwerfen",
            Zh => "放弃更改",
        },
        Msg::DiscardConfigPrompt => match lang {
            En => "You have unsaved changes. Discard them and return home?",
            It => "Ci sono modifiche non salvate. Vuoi scartarle e tornare al menu principale?",
            Es => "Hay cambios sin guardar. ¿Descartarlos y volver al inicio?",
            Fr => {
                "Des modifications ne sont pas enregistrées. Les annuler et revenir à l'accueil ?"
            }
            De => "Es gibt ungespeicherte Änderungen. Verwerfen und zum Hauptmenü zurückkehren?",
            Zh => "有未保存的更改。是否放弃并返回主页？",
        },
        Msg::CancelAnalysisTitle => match lang {
            En => "Cancel Analysis",
            It => "Annulla analisi",
            Es => "Cancelar análisis",
            Fr => "Annuler l'analyse",
            De => "Analyse abbrechen",
            Zh => "取消分析",
        },
        Msg::CancelAnalysisPrompt => match lang {
            En => "Cancel the remaining file analysis?",
            It => "Annullare l’analisi dei file rimanenti?",
            Es => "¿Cancelar el análisis de los archivos restantes?",
            Fr => "Annuler l’analyse des fichiers restants ?",
            De => "Die Analyse der übrigen Dateien abbrechen?",
            Zh => "取消其余文件的分析吗？",
        },
        Msg::Yes => match lang {
            En => "Yes",
            It => "Sì",
            Es => "Sí",
            Fr => "Oui",
            De => "Ja",
            Zh => "是",
        },
        Msg::No => match lang {
            En | It | Es => "No",
            Fr => "Non",
            De => "Nein",
            Zh => "否",
        },

        // ── Config screen ────────────────────────────────────────────────────
        Msg::Settings => match lang {
            En => "Settings",
            It => "Impostazioni",
            Es => "Ajustes",
            Fr => "Paramètres",
            De => "Einstellungen",
            Zh => "设置",
        },
        Msg::EnterToEdit => match lang {
            En => " (Enter to edit)",
            It => " (Invio per modificare)",
            Es => " (Enter para editar)",
            Fr => " (Entrée pour modifier)",
            De => " (Enter zum Bearbeiten)",
            Zh => "（按 Enter 编辑）",
        },
        Msg::SavedExclaim => match lang {
            En => "Saved!",
            It => "Salvato!",
            Es => "¡Guardado!",
            Fr => "Enregistré !",
            De => "Gespeichert!",
            Zh => "已保存！",
        },
        Msg::SaveFailed => match lang {
            En => "Save failed",
            It => "Salvataggio non riuscito",
            Es => "Error al guardar",
            Fr => "Échec de l'enregistrement",
            De => "Speichern fehlgeschlagen",
            Zh => "保存失败",
        },
        Msg::UnsavedChanges => match lang {
            En => "Unsaved changes",
            It => "Modifiche non salvate",
            Es => "Cambios sin guardar",
            Fr => "Modifications non enregistrées",
            De => "Ungespeicherte Änderungen",
            Zh => "未保存的更改",
        },
        Msg::InvalidAddress => match lang {
            En => "Enter a valid IP address",
            It => "Inserisci un indirizzo IP valido",
            Es => "Introduce una dirección IP válida",
            Fr => "Saisissez une adresse IP valide",
            De => "Geben Sie eine gültige IP-Adresse ein",
            Zh => "请输入有效的 IP 地址",
        },
        Msg::InvalidPort => match lang {
            En => "Enter a port from 1 to 65535",
            It => "Inserisci una porta da 1 a 65535",
            Es => "Introduce un puerto entre 1 y 65535",
            Fr => "Saisissez un port compris entre 1 et 65535",
            De => "Geben Sie einen Port von 1 bis 65535 ein",
            Zh => "请输入 1 到 65535 之间的端口",
        },
        Msg::CfgGroupTracks | Msg::WebTracksTitle => match lang {
            En => "Tracks",
            It => "Tracce",
            Es => "Pistas",
            Fr => "Pistes",
            De => "Spuren",
            Zh => "轨道",
        },
        Msg::CfgGroupDisc => match lang {
            En | De => "Disc",
            It | Es => "Disco",
            Fr => "Disque",
            Zh => "光盘",
        },
        Msg::CfgVmafThreshold => match lang {
            En => "VMAF Threshold",
            It => "Soglia VMAF",
            Es => "Umbral VMAF",
            Fr => "Seuil VMAF",
            De => "VMAF-Schwelle",
            Zh => "VMAF 阈值",
        },
        Msg::CfgVmafEnabled => match lang {
            En => "VMAF Enabled",
            It => "VMAF attivo",
            Es => "VMAF activado",
            Fr => "VMAF activé",
            De => "VMAF aktiviert",
            Zh => "启用 VMAF",
        },
        Msg::CfgDeleteSource => match lang {
            En => "Delete Source if VMAF Passes",
            It => "Elimina origine se VMAF OK",
            Es => "Eliminar origen si VMAF OK",
            Fr => "Supprimer la source si VMAF OK",
            De => "Quelle löschen bei VMAF-Erfolg",
            Zh => "VMAF 达标后删除源文件",
        },
        Msg::CfgSvtPreset => match lang {
            En => "SVT-AV1 Preset",
            It => "Preset SVT-AV1",
            Es => "Preajuste SVT-AV1",
            Fr => "Préréglage SVT-AV1",
            De => "SVT-AV1-Voreinstellung",
            Zh => "SVT-AV1 预设",
        },
        Msg::CfgNvencPreset => match lang {
            En => "NVENC Preset",
            It => "Preset NVENC",
            Es => "Preajuste NVENC",
            Fr => "Préréglage NVENC",
            De => "NVENC-Voreinstellung",
            Zh => "NVENC 预设",
        },
        Msg::CfgQualityPreset => match lang {
            En => "Quality Preset",
            It => "Preset qualità",
            Es => "Preajuste de calidad",
            Fr => "Préréglage de qualité",
            De => "Qualitätsvoreinstellung",
            Zh => "质量预设",
        },
        Msg::QpLow => match lang {
            En => "Low",
            It => "Bassa",
            Es => "Baja",
            Fr => "Basse",
            De => "Niedrig",
            Zh => "低",
        },
        Msg::QpMedium => match lang {
            En => "Medium",
            It | Es => "Media",
            Fr => "Moyenne",
            De => "Mittel",
            Zh => "中",
        },
        Msg::QpHigh => match lang {
            En => "High",
            It | Es => "Alta",
            Fr => "Haute",
            De => "Hoch",
            Zh => "高",
        },
        Msg::QpCustom => match lang {
            En => "Custom",
            It => "Personalizzato",
            Es => "Personalizado",
            Fr => "Personnalisé",
            De => "Benutzerdefiniert",
            Zh => "自定义",
        },
        // Resolution/rate-factor labels are technical and stay language-invariant.
        Msg::CfgRfSd => "RF SD",
        Msg::CfgRfHd => "RF HD (720p)",
        Msg::CfgRfFullHd => "RF 1080p SDR",
        Msg::CfgRfFullHdHdr => "RF 1080p HDR",
        Msg::CfgRfFullHdDv => "RF 1080p DV",
        Msg::CfgRfUhd => "RF 4K SDR",
        Msg::CfgRfUhdHdr => "RF 4K HDR",
        Msg::CfgRfUhdDv => "RF 4K DV",
        Msg::CfgFilmGrain => match lang {
            En => "Film Grain",
            It => "Grana pellicola",
            Es => "Grano de película",
            Fr => "Grain de film",
            De => "Filmkorn",
            Zh => "胶片颗粒",
        },
        Msg::CfgOutputSuffix => match lang {
            En => "Output Suffix",
            It => "Suffisso output",
            Es => "Sufijo de salida",
            Fr => "Suffixe de sortie",
            De => "Ausgabe-Suffix",
            Zh => "输出后缀",
        },
        Msg::CfgOutputContainer => match lang {
            En => "Output Container",
            It => "Contenitore output",
            Es => "Contenedor de salida",
            Fr => "Conteneur de sortie",
            De => "Ausgabecontainer",
            Zh => "输出容器",
        },
        Msg::CfgSameDirectory => match lang {
            En => "Same Directory Output",
            It => "Output nella stessa cartella",
            Es => "Salida en el mismo directorio",
            Fr => "Sortie dans le même dossier",
            De => "Ausgabe im selben Verzeichnis",
            Zh => "输出到同一目录",
        },
        Msg::CfgAudioLanguages => match lang {
            En => "Preferred Audio Languages",
            It => "Lingue audio preferite",
            Es => "Idiomas de audio preferidos",
            Fr => "Langues audio préférées",
            De => "Bevorzugte Audiosprachen",
            Zh => "首选音频语言",
        },
        Msg::CfgSubtitleLanguages => match lang {
            En => "Preferred Subtitle Languages",
            It => "Lingue sottotitoli preferite",
            Es => "Idiomas de subtítulos preferidos",
            Fr => "Langues de sous-titres préférées",
            De => "Bevorzugte Untertitelsprachen",
            Zh => "首选字幕语言",
        },
        Msg::CfgLanguage => match lang {
            En => "Language",
            It => "Lingua",
            Es => "Idioma",
            Fr => "Langue",
            De => "Sprache",
            Zh => "语言",
        },
        Msg::CfgDaemonEnabled => match lang {
            En => "Web Daemon Enabled",
            It => "Daemon web abilitato",
            Es => "Daemon web habilitado",
            Fr => "Daemon web activé",
            De => "Web-Daemon aktiviert",
            Zh => "启用 Web 守护进程",
        },
        Msg::CfgDaemonAutostart => match lang {
            En => "Run at Startup",
            It => "Avvia all'accesso",
            Es => "Ejecutar al iniciar",
            Fr => "Démarrer à la connexion",
            De => "Beim Anmelden starten",
            Zh => "登录时启动",
        },
        Msg::CfgDaemonAutostartHint => match lang {
            En => {
                "Starts the web UI at login. A waiting queue will encode when the machine comes up. --stop lasts until the next login."
            }
            It => {
                "Avvia l'interfaccia web all'accesso. Una coda in attesa verrà codificata all'avvio. --stop vale fino al prossimo accesso."
            }
            Es => {
                "Arranca la interfaz web al iniciar sesión. Una cola pendiente se codificará al encender. --stop dura hasta el próximo inicio de sesión."
            }
            Fr => {
                "Démarre l'interface web à la connexion. Une file d'attente sera encodée au démarrage. --stop tient jusqu'à la prochaine connexion."
            }
            De => {
                "Startet die Web-UI bei der Anmeldung. Eine wartende Warteschlange wird beim Hochfahren kodiert. --stop gilt bis zur nächsten Anmeldung."
            }
            Zh => "登录时启动 Web 界面。等待中的队列会在开机后开始编码。--stop 只持续到下次登录。",
        },
        Msg::CfgDaemonBindAddress => match lang {
            En => "Daemon Bind Address",
            It => "Indirizzo di ascolto daemon",
            Es => "Dirección de escucha del daemon",
            Fr => "Adresse d'écoute du daemon",
            De => "Daemon-Bindungsadresse",
            Zh => "守护进程监听地址",
        },
        Msg::CfgDaemonPort => match lang {
            En => "Daemon Port",
            It => "Porta daemon",
            Es => "Puerto del daemon",
            Fr => "Port du daemon",
            De => "Daemon-Port",
            Zh => "守护进程端口",
        },
        Msg::CfgDaemonBrowseRoot => match lang {
            En => "Daemon Browse Root",
            It => "Cartella base daemon",
            Es => "Carpeta base del daemon",
            Fr => "Dossier racine du daemon",
            De => "Daemon-Basisordner",
            Zh => "守护进程浏览根目录",
        },
        Msg::CfgMakemkvconPath => match lang {
            En => "MakeMKV Executable",
            It => "Eseguibile MakeMKV",
            Es => "Ejecutable de MakeMKV",
            Fr => "Exécutable MakeMKV",
            De => "MakeMKV-Programm",
            Zh => "MakeMKV 可执行文件",
        },
        Msg::CfgStagingDirectory => match lang {
            En => "Disc Staging Directory",
            It => "Cartella di staging dischi",
            Es => "Carpeta temporal de discos",
            Fr => "Dossier temporaire des disques",
            De => "Zwischenordner für Discs",
            Zh => "光盘暂存目录",
        },
        Msg::CfgDaemonAuthToken => match lang {
            En => "Daemon Access Token",
            It => "Token di accesso daemon",
            Es => "Token de acceso del daemon",
            Fr => "Jeton d'accès du daemon",
            De => "Daemon-Zugriffstoken",
            Zh => "守护进程访问令牌",
        },
        Msg::DaemonDisabledError => match lang {
            En => {
                "Daemon mode is disabled. Enable it in Settings or set enabled = true under [daemon] in config.toml."
            }
            It => {
                "La modalità daemon è disabilitata. Abilitala nelle Impostazioni o imposta enabled = true sotto [daemon] in config.toml."
            }
            Es => {
                "El modo daemon está deshabilitado. Habilítalo en Configuración o establece enabled = true bajo [daemon] en config.toml."
            }
            Fr => {
                "Le mode daemon est désactivé. Activez-le dans les Paramètres ou définissez enabled = true sous [daemon] dans config.toml."
            }
            De => {
                "Der Daemon-Modus ist deaktiviert. Aktiviere ihn in den Einstellungen oder setze enabled = true unter [daemon] in config.toml."
            }
            Zh => {
                "守护进程模式已禁用。请在设置中启用，或在 config.toml 的 [daemon] 下设置 enabled = true。"
            }
        },
        Msg::DaemonListening => match lang {
            En => "Web UI listening on",
            It => "Interfaccia web in ascolto su",
            Es => "Interfaz web escuchando en",
            Fr => "Interface web à l'écoute sur",
            De => "Web-UI lauscht auf",
            Zh => "Web 界面监听于",
        },
        Msg::EncoderUnavailable => match lang {
            En => {
                "Warning: the selected encoder is missing from this FFmpeg build; every encode will fail."
            }
            It => {
                "Attenzione: il codificatore selezionato non è presente in questa build di FFmpeg; ogni conversione fallirà."
            }
            Es => {
                "Aviso: el codificador seleccionado no está en esta compilación de FFmpeg; todas las conversiones fallarán."
            }
            Fr => {
                "Attention : l'encodeur sélectionné est absent de cette version de FFmpeg ; tous les encodages échoueront."
            }
            De => {
                "Warnung: Der gewählte Encoder fehlt in diesem FFmpeg-Build; jede Kodierung wird fehlschlagen."
            }
            Zh => "警告：所选编码器不在此 FFmpeg 构建中；所有转换都将失败。",
        },
        Msg::DaemonTokenGenerated => match lang {
            En => {
                "No strong access token was set, so one has been generated and saved to the config. Open the URL below to authorise your browser."
            }
            It => {
                "Non era impostato un token di accesso sicuro: ne è stato generato uno e salvato nella configurazione. Apri l'URL qui sotto per autorizzare il browser."
            }
            Es => {
                "No había un token de acceso seguro, así que se ha generado uno y guardado en la configuración. Abre la URL de abajo para autorizar tu navegador."
            }
            Fr => {
                "Aucun jeton d'accès robuste n'était défini : un jeton a été généré et enregistré dans la configuration. Ouvrez l'URL ci-dessous pour autoriser votre navigateur."
            }
            De => {
                "Es war kein starkes Zugriffstoken gesetzt, daher wurde eines erzeugt und in der Konfiguration gespeichert. Öffnen Sie die URL unten, um Ihren Browser zu autorisieren."
            }
            Zh => {
                "未设置安全的访问令牌，已生成一个并保存到配置中。请打开下方的网址以授权您的浏览器。"
            }
        },
        Msg::DaemonPublicHttp => match lang {
            En => {
                "Warning: this network-facing daemon uses plain HTTP. Put it behind HTTPS or use it only on a trusted network."
            }
            It => {
                "Attenzione: questo daemon esposto in rete usa HTTP non cifrato. Proteggilo con HTTPS o usalo solo su una rete fidata."
            }
            Es => {
                "Aviso: este daemon expuesto a la red usa HTTP sin cifrar. Colócalo detrás de HTTPS o úsalo solo en una red de confianza."
            }
            Fr => {
                "Attention : ce daemon exposé au réseau utilise HTTP sans chiffrement. Placez-le derrière HTTPS ou limitez-le à un réseau fiable."
            }
            De => {
                "Warnung: Dieser im Netzwerk erreichbare Daemon verwendet unverschlüsseltes HTTP. Schalten Sie HTTPS davor oder nutzen Sie ihn nur in einem vertrauenswürdigen Netz."
            }
            Zh => {
                "警告：此网络守护进程使用未加密的 HTTP。请在前端配置 HTTPS，或仅在可信网络中使用。"
            }
        },
        Msg::DaemonShuttingDown => match lang {
            En => "Shutting down, stopping current encode...",
            It => "Arresto in corso, interruzione della codifica corrente...",
            Es => "Apagando, deteniendo la codificación actual...",
            Fr => "Arrêt en cours, interruption de l'encodage actuel...",
            De => "Wird beendet, aktuelle Kodierung wird gestoppt...",
            Zh => "正在关闭，停止当前编码...",
        },
        Msg::DaemonStarted => match lang {
            En => "Daemon started in the background",
            It => "Daemon avviato in background",
            Es => "Daemon iniciado en segundo plano",
            Fr => "Daemon démarré en arrière-plan",
            De => "Daemon im Hintergrund gestartet",
            Zh => "守护进程已在后台启动",
        },
        Msg::DaemonStartFailed => match lang {
            En => "Daemon failed to start; see the log at",
            It => "Avvio del daemon non riuscito; vedi il log in",
            Es => "El daemon no pudo iniciarse; consulta el registro en",
            Fr => "Échec du démarrage du daemon ; consultez le journal dans",
            De => "Daemon konnte nicht gestartet werden; siehe Log unter",
            Zh => "守护进程启动失败；请查看日志：",
        },
        Msg::DaemonAlreadyRunning => match lang {
            En => "Daemon is already running",
            It => "Il daemon è già in esecuzione",
            Es => "El daemon ya está en ejecución",
            Fr => "Le daemon est déjà en cours d'exécution",
            De => "Daemon läuft bereits",
            Zh => "守护进程已在运行",
        },
        Msg::DaemonRunning => match lang {
            En => "Daemon is running",
            It => "Il daemon è in esecuzione",
            Es => "El daemon está en ejecución",
            Fr => "Le daemon est en cours d'exécution",
            De => "Daemon läuft",
            Zh => "守护进程正在运行",
        },
        Msg::DaemonNotRunning => match lang {
            En => "Daemon is not running",
            It => "Il daemon non è in esecuzione",
            Es => "El daemon no está en ejecución",
            Fr => "Le daemon n'est pas en cours d'exécution",
            De => "Daemon läuft nicht",
            Zh => "守护进程未运行",
        },
        Msg::DaemonStopped => match lang {
            En => "Daemon stopped",
            It => "Daemon arrestato",
            Es => "Daemon detenido",
            Fr => "Daemon arrêté",
            De => "Daemon beendet",
            Zh => "守护进程已停止",
        },
        Msg::DaemonStopFailed => match lang {
            En => "Failed to stop the daemon:",
            It => "Impossibile arrestare il daemon:",
            Es => "No se pudo detener el daemon:",
            Fr => "Échec de l'arrêt du daemon :",
            De => "Daemon konnte nicht beendet werden:",
            Zh => "无法停止守护进程：",
        },
        Msg::DaemonStopHint => match lang {
            En => "Stop it with: av1converter --stop",
            It => "Arrestalo con: av1converter --stop",
            Es => "Deténlo con: av1converter --stop",
            Fr => "Arrêtez-le avec : av1converter --stop",
            De => "Beenden mit: av1converter --stop",
            Zh => "使用 av1converter --stop 停止",
        },
        Msg::DaemonServiceInstalled => match lang {
            En => "Daemon will start at login",
            It => "Il daemon si avvierà all'accesso",
            Es => "El daemon se iniciará al iniciar sesión",
            Fr => "Le daemon démarrera à la connexion",
            De => "Daemon startet bei der Anmeldung",
            Zh => "守护进程将在登录时启动",
        },
        Msg::DaemonServiceUninstalled => match lang {
            En => "Daemon will no longer start at login",
            It => "Il daemon non si avvierà più all'accesso",
            Es => "El daemon ya no se iniciará al iniciar sesión",
            Fr => "Le daemon ne démarrera plus à la connexion",
            De => "Daemon startet nicht mehr bei der Anmeldung",
            Zh => "守护进程不再于登录时启动",
        },
        Msg::DaemonServiceFailed => match lang {
            En => "Could not update login autostart:",
            It => "Impossibile aggiornare l'avvio automatico:",
            Es => "No se pudo actualizar el inicio automático:",
            Fr => "Impossible de mettre à jour le démarrage automatique :",
            De => "Autostart konnte nicht geändert werden:",
            Zh => "无法更新登录自启动：",
        },
        Msg::DaemonServiceUnsupported => match lang {
            En => "Starting at login is only supported on Linux (systemd) and macOS",
            It => "L'avvio all'accesso è supportato solo su Linux (systemd) e macOS",
            Es => "El inicio de sesión automático solo está disponible en Linux (systemd) y macOS",
            Fr => {
                "Le démarrage à la connexion n'est pris en charge que sous Linux (systemd) et macOS"
            }
            De => "Start bei Anmeldung wird nur unter Linux (systemd) und macOS unterstützt",
            Zh => "登录时启动仅支持 Linux（systemd）和 macOS",
        },
        Msg::DaemonServiceLingerHint => match lang {
            En => "On a headless machine, run: loginctl enable-linger $USER",
            It => "Su una macchina senza sessione grafica: loginctl enable-linger $USER",
            Es => "En una máquina sin sesión gráfica: loginctl enable-linger $USER",
            Fr => "Sur une machine sans session graphique : loginctl enable-linger $USER",
            De => "Auf einem Rechner ohne grafische Sitzung: loginctl enable-linger $USER",
            Zh => "在无图形会话的机器上请运行：loginctl enable-linger $USER",
        },
        Msg::DaemonAutostartOn => match lang {
            En => "Starts at login",
            It => "Si avvia all'accesso",
            Es => "Se inicia al iniciar sesión",
            Fr => "Démarre à la connexion",
            De => "Startet bei der Anmeldung",
            Zh => "登录时启动",
        },
        Msg::DaemonAutostartOff => match lang {
            En => "Does not start at login",
            It => "Non si avvia all'accesso",
            Es => "No se inicia al iniciar sesión",
            Fr => "Ne démarre pas à la connexion",
            De => "Startet nicht bei der Anmeldung",
            Zh => "登录时不启动",
        },
        // ── Web UI ───────────────────────────────────────────────────────────
        Msg::WebTabQueue => match lang {
            En => "Queue",
            It => "Coda",
            Es => "Cola",
            Fr => "File",
            De => "Warteschlange",
            Zh => "队列",
        },
        Msg::WebOffline => match lang {
            En => "Daemon unreachable — retrying…",
            It => "Daemon irraggiungibile — nuovo tentativo…",
            Es => "Daemon inaccesible — reintentando…",
            Fr => "Daemon injoignable — nouvelle tentative…",
            De => "Daemon nicht erreichbar — neuer Versuch…",
            Zh => "无法连接守护进程 — 正在重试…",
        },
        Msg::WebIdle => match lang {
            En => "Idle",
            It => "Inattivo",
            Es => "Inactivo",
            Fr => "Inactif",
            De => "Bereit",
            Zh => "空闲",
        },
        Msg::WebStatInQueue => match lang {
            En => "In queue",
            It => "In coda",
            Es => "En cola",
            Fr => "Dans la file",
            De => "In Warteschlange",
            Zh => "队列中",
        },
        Msg::WebCurrentFile => match lang {
            En => "Current file",
            It => "File corrente",
            Es => "Archivo actual",
            Fr => "Fichier en cours",
            De => "Aktuelle Datei",
            Zh => "当前文件",
        },
        Msg::WebIdleNothing => match lang {
            En => "Idle — nothing encoding",
            It => "Inattivo — nessuna codifica",
            Es => "Inactivo — sin codificación",
            Fr => "Inactif — aucun encodage",
            De => "Bereit — keine Kodierung",
            Zh => "空闲 — 无编码任务",
        },
        Msg::WebOverallProgress => match lang {
            En => "Overall progress",
            It => "Avanzamento totale",
            Es => "Progreso total",
            Fr => "Progression totale",
            De => "Gesamtfortschritt",
            Zh => "总进度",
        },
        Msg::WebAddFile => match lang {
            En | It => "+ File",
            Es => "+ Archivo",
            Fr => "+ Fichier",
            De => "+ Datei",
            Zh => "+ 文件",
        },
        Msg::WebAddFolder => match lang {
            En => "+ Folder",
            It => "+ Cartella",
            Es => "+ Carpeta",
            Fr => "+ Dossier",
            De => "+ Ordner",
            Zh => "+ 文件夹",
        },
        Msg::WebAddFolderRecursive => match lang {
            En => "+ Folder (recursive)",
            It => "+ Cartella (ricorsiva)",
            Es => "+ Carpeta (recursiva)",
            Fr => "+ Dossier (récursif)",
            De => "+ Ordner (rekursiv)",
            Zh => "+ 文件夹（递归）",
        },
        Msg::WebClearFinished => match lang {
            En => "Clear finished",
            It => "Rimuovi completati",
            Es => "Quitar completados",
            Fr => "Effacer les terminés",
            De => "Fertige entfernen",
            Zh => "清除已完成",
        },
        Msg::WebSize => match lang {
            En => "Size",
            It => "Dimensione",
            Es => "Tamaño",
            Fr => "Taille",
            De => "Größe",
            Zh => "大小",
        },
        Msg::WebSaved => match lang {
            En => "Saved",
            It => "Risparmio",
            Es => "Ahorro",
            Fr => "Économisé",
            De => "Gespart",
            Zh => "已节省",
        },
        Msg::WebQueueEmpty => match lang {
            En => "Queue is empty — add files to start encoding",
            It => "La coda è vuota — aggiungi file per iniziare",
            Es => "La cola está vacía — añade archivos para empezar",
            Fr => "La file est vide — ajoutez des fichiers pour commencer",
            De => "Warteschlange leer — Dateien hinzufügen, um zu starten",
            Zh => "队列为空 — 添加文件以开始编码",
        },
        Msg::WebTagRemux => match lang {
            En | It | Es | Fr => "remux",
            De => "Remux",
            Zh => "重封装",
        },
        Msg::WebVmafFailed => match lang {
            En => "VMAF failed",
            It => "VMAF fallito",
            Es => "VMAF falló",
            Fr => "VMAF échoué",
            De => "VMAF fehlgeschlagen",
            Zh => "VMAF 失败",
        },
        Msg::WebLowVmaf => match lang {
            En => "Low VMAF",
            It => "VMAF basso",
            Es => "VMAF bajo",
            Fr => "VMAF faible",
            De => "Niedriger VMAF",
            Zh => "VMAF 偏低",
        },
        Msg::WebSelectFolderRecursive => match lang {
            En => "Select a folder (recursive)",
            It => "Seleziona una cartella (ricorsiva)",
            Es => "Seleccionar una carpeta (recursiva)",
            Fr => "Sélectionner un dossier (récursif)",
            De => "Ordner wählen (rekursiv)",
            Zh => "选择文件夹（递归）",
        },
        Msg::WebHiddenFiles => match lang {
            En => "Hidden files",
            It => "File nascosti",
            Es => "Archivos ocultos",
            Fr => "Fichiers cachés",
            De => "Versteckte Dateien",
            Zh => "隐藏文件",
        },
        Msg::WebParentDirectory => match lang {
            En => "Parent directory",
            It => "Cartella superiore",
            Es => "Carpeta superior",
            Fr => "Dossier parent",
            De => "Übergeordneter Ordner",
            Zh => "上级目录",
        },
        Msg::WebKindFolder => match lang {
            En => "folder",
            It => "cartella",
            Es => "carpeta",
            Fr => "dossier",
            De => "Ordner",
            Zh => "文件夹",
        },
        Msg::WebKindVideoFile => match lang {
            En => "video file",
            It => "file video",
            Es => "archivo de video",
            Fr => "fichier vidéo",
            De => "Videodatei",
            Zh => "视频文件",
        },
        Msg::WebKindFile => match lang {
            En | It => "file",
            Es => "archivo",
            Fr => "fichier",
            De => "Datei",
            Zh => "文件",
        },
        Msg::WebKindSymlink => match lang {
            En => "symlink",
            It => "collegamento",
            Es => "enlace simbólico",
            Fr => "lien symbolique",
            De => "Symlink",
            Zh => "符号链接",
        },
        Msg::WebKindNotSelectable => match lang {
            En => "not selectable",
            It => "non selezionabile",
            Es => "no seleccionable",
            Fr => "non sélectionnable",
            De => "nicht auswählbar",
            Zh => "不可选择",
        },
        Msg::WebTracksHint => match lang {
            En => "Choose audio and subtitle tracks",
            It => "Scegli le tracce audio e dei sottotitoli",
            Es => "Elegir pistas de audio y subtítulos",
            Fr => "Choisir les pistes audio et de sous-titres",
            De => "Audio- und Untertitelspuren wählen",
            Zh => "选择音频和字幕轨道",
        },
        Msg::WebCloseTracks => match lang {
            En => "Close track selection",
            It => "Chiudi la selezione delle tracce",
            Es => "Cerrar la selección de pistas",
            Fr => "Fermer la sélection des pistes",
            De => "Spurauswahl schließen",
            Zh => "关闭轨道选择",
        },
        Msg::WebTracksLocked => match lang {
            En => "This job is already encoding — tracks cannot be changed.",
            It => "Questo lavoro è già in codifica — le tracce non sono modificabili.",
            Es => "Este trabajo ya se está codificando — las pistas no se pueden cambiar.",
            Fr => "Cette tâche est déjà en cours d'encodage — les pistes ne sont plus modifiables.",
            De => "Dieser Auftrag wird bereits kodiert — Spuren können nicht geändert werden.",
            Zh => "此任务正在编码 — 无法更改轨道。",
        },
        Msg::WebTracksUpdated => match lang {
            En => "Tracks updated",
            It => "Tracce aggiornate",
            Es => "Pistas actualizadas",
            Fr => "Pistes mises à jour",
            De => "Spuren aktualisiert",
            Zh => "轨道已更新",
        },
        Msg::WebApplyRemaining => match lang {
            En => "Apply to remaining files",
            It => "Applica ai file rimanenti",
            Es => "Aplicar a los archivos restantes",
            Fr => "Appliquer aux fichiers restants",
            De => "Auf verbleibende Dateien anwenden",
            Zh => "应用于剩余文件",
        },
        Msg::WebTracksApplied => match lang {
            En => "Applied to {n} files",
            It => "Applicato a {n} file",
            Es => "Aplicado a {n} archivos",
            Fr => "Appliqué à {n} fichiers",
            De => "Auf {n} Dateien angewendet",
            Zh => "已应用于 {n} 个文件",
        },
        Msg::WebNoAudioTracks => match lang {
            En => "No audio tracks",
            It => "Nessuna traccia audio",
            Es => "Sin pistas de audio",
            Fr => "Aucune piste audio",
            De => "Keine Audiospuren",
            Zh => "无音频轨道",
        },
        Msg::WebNoSubtitleTracks => match lang {
            En => "No subtitle tracks",
            It => "Nessuna traccia sottotitoli",
            Es => "Sin pistas de subtítulos",
            Fr => "Aucune piste de sous-titres",
            De => "Keine Untertitelspuren",
            Zh => "无字幕轨道",
        },
        Msg::WebSelectAll => match lang {
            En => "Select all",
            It => "Seleziona tutto",
            Es => "Seleccionar todo",
            Fr => "Tout sélectionner",
            De => "Alle auswählen",
            Zh => "全选",
        },
        Msg::WebClearAll => match lang {
            En => "Clear all",
            It => "Deseleziona tutto",
            Es => "Deseleccionar todo",
            Fr => "Tout désélectionner",
            De => "Auswahl aufheben",
            Zh => "全部取消",
        },
        Msg::WebOptions => match lang {
            En | Fr => "Options",
            It => "Opzioni",
            Es => "Opciones",
            De => "Optionen",
            Zh => "选项",
        },
        Msg::WebTrackExclude => match lang {
            En => "Exclude",
            It => "Escludi",
            Es => "Excluir",
            Fr => "Exclure",
            De => "Ausschließen",
            Zh => "排除",
        },
        Msg::WebAlreadyOpusCopied => match lang {
            En => "already Opus — copied",
            It => "già Opus — copiata",
            Es => "ya es Opus — copiada",
            Fr => "déjà Opus — copiée",
            De => "bereits Opus — kopiert",
            Zh => "已是 Opus — 直接复制",
        },
        Msg::WebRemuxHint => match lang {
            En => "Repackage without re-encoding the video. Default for sources already in AV1.",
            It => {
                "Ricontenitorizza senza ricodificare il video. Predefinito per sorgenti già in AV1."
            }
            Es => "Reempaqueta sin recodificar el video. Predeterminado para fuentes ya en AV1.",
            Fr => "Réencapsule sans réencoder la vidéo. Par défaut pour les sources déjà en AV1.",
            De => {
                "Neu verpacken, ohne das Video neu zu kodieren. Standard für Quellen, die bereits AV1 sind."
            }
            Zh => "重新封装而不重新编码视频。已是 AV1 的源默认使用此项。",
        },
        Msg::WebDolbyVision => match lang {
            En | It | Es | Fr | De => "Dolby Vision",
            Zh => "杜比视界",
        },
        Msg::WebDvRemuxHint => match lang {
            En => "A remux keeps the source stream untouched, Dolby Vision included.",
            It => "Un remux lascia intatto il flusso sorgente, Dolby Vision compreso.",
            Es => "Un remux deja intacto el flujo de origen, incluido Dolby Vision.",
            Fr => "Un remux laisse le flux source intact, Dolby Vision compris.",
            De => "Ein Remux lässt den Quellstream unangetastet, samt Dolby Vision.",
            Zh => "重封装会保持源流不变，包括杜比视界。",
        },
        Msg::WebDvSourceHint => match lang {
            En => "Dolby Vision {profile} source.",
            It => "Sorgente Dolby Vision {profile}.",
            Es => "Fuente Dolby Vision {profile}.",
            Fr => "Source Dolby Vision {profile}.",
            De => "Dolby-Vision-Quelle {profile}.",
            Zh => "杜比视界 {profile} 源。",
        },
        Msg::WebDvProfile => match lang {
            En => "profile {n}",
            It => "profilo {n}",
            Es => "perfil {n}",
            Fr => "profil {n}",
            De => "Profil {n}",
            Zh => "配置 {n}",
        },
        Msg::WebCancelling => match lang {
            En => "Cancelling…",
            It => "Annullamento…",
            Es => "Cancelando…",
            Fr => "Annulation…",
            De => "Wird abgebrochen…",
            Zh => "正在取消…",
        },
        Msg::WebRemovedFinished => match lang {
            En => "Removed {n} finished job(s)",
            It => "Rimossi {n} lavori completati",
            Es => "Se quitaron {n} trabajos completados",
            Fr => "{n} tâche(s) terminée(s) retirée(s)",
            De => "{n} fertige Aufträge entfernt",
            Zh => "已移除 {n} 个已完成任务",
        },
        Msg::WebAddedFiles => match lang {
            En => "Added {n} file(s) to the queue",
            It => "Aggiunti {n} file alla coda",
            Es => "Se añadieron {n} archivos a la cola",
            Fr => "{n} fichier(s) ajouté(s) à la file",
            De => "{n} Datei(en) zur Warteschlange hinzugefügt",
            Zh => "已将 {n} 个文件加入队列",
        },
        Msg::WebAlreadyQueued => match lang {
            En => "{n} already queued",
            It => "{n} già in coda",
            Es => "{n} ya en cola",
            Fr => "{n} déjà dans la file",
            De => "{n} bereits in der Warteschlange",
            Zh => "{n} 个已在队列中",
        },
        Msg::WebNothingAdded => match lang {
            En => "Nothing added — {n} file(s) already queued",
            It => "Nessuna aggiunta — {n} file già in coda",
            Es => "No se añadió nada — {n} archivos ya en cola",
            Fr => "Rien d'ajouté — {n} fichier(s) déjà dans la file",
            De => "Nichts hinzugefügt — {n} Datei(en) bereits in der Warteschlange",
            Zh => "未添加 — {n} 个文件已在队列中",
        },
        Msg::WebUnauthorized => match lang {
            En => "Unauthorized — open the UI with #token=… from your config",
            It => "Non autorizzato — apri l'interfaccia con #token=… dalla configurazione",
            Es => "No autorizado — abre la interfaz con #token=… de tu configuración",
            Fr => "Non autorisé — ouvrez l'interface avec #token=… depuis votre configuration",
            De => "Nicht autorisiert — Oberfläche mit #token=… aus der Konfiguration öffnen",
            Zh => "未授权 — 请使用配置中的 #token=… 打开界面",
        },
        Msg::WebRemoveFromQueue => match lang {
            En => "Remove from queue",
            It => "Rimuovi dalla coda",
            Es => "Quitar de la cola",
            Fr => "Retirer de la file",
            De => "Aus der Warteschlange entfernen",
            Zh => "从队列中移除",
        },
        Msg::WebGroupGeneral => match lang {
            En | Es => "General",
            It => "Generale",
            Fr => "Général",
            De => "Allgemein",
            Zh => "常规",
        },
        Msg::WebGroupQuality => match lang {
            En => "Quality",
            It => "Qualità",
            Es => "Calidad",
            Fr => "Qualité",
            De => "Qualität",
            Zh => "质量",
        },
        Msg::WebGroupPerformance => match lang {
            En | Fr => "Performance",
            It => "Prestazioni",
            Es => "Rendimiento",
            De => "Leistung",
            Zh => "性能",
        },
        Msg::WebGroupOutput => match lang {
            En | It => "Output",
            Es => "Salida",
            Fr => "Sortie",
            De => "Ausgabe",
            Zh => "输出",
        },
        Msg::WebGroupAudio => match lang {
            En | It | Es | Fr | De => "Audio",
            Zh => "音频",
        },
        Msg::WebGroupDaemon => match lang {
            En | It | Es | Fr | De => "Daemon",
            Zh => "守护进程",
        },
        Msg::WebGroupDisc => match lang {
            En => "Disc ripping",
            It => "Estrazione dischi",
            Es => "Extracción de discos",
            Fr => "Extraction de disques",
            De => "Disc-Rippen",
            Zh => "光盘翻录",
        },
        Msg::WebGroupRateFactors => match lang {
            En => "Rate factors",
            It => "Fattori di qualità",
            Es => "Factores de calidad",
            Fr => "Facteurs de qualité",
            De => "Ratenfaktoren",
            Zh => "码率因子",
        },
        Msg::WebCfgOutputDirectory => match lang {
            En => "Output directory (if not same)",
            It => "Cartella di output (se diversa)",
            Es => "Carpeta de salida (si no es la misma)",
            Fr => "Dossier de sortie (si différent)",
            De => "Ausgabeordner (falls abweichend)",
            Zh => "输出目录（若不同）",
        },
        Msg::WebCfgSelectAllFallback => match lang {
            En => "Select all tracks as fallback",
            It => "Seleziona tutte le tracce come ripiego",
            Es => "Seleccionar todas las pistas como alternativa",
            Fr => "Sélectionner toutes les pistes par défaut",
            De => "Alle Spuren als Rückfall auswählen",
            Zh => "回退时选择所有轨道",
        },
        Msg::WebCfgAudioDefault => match lang {
            En => "New files default to",
            It => "Impostazione predefinita per i nuovi file",
            Es => "Valor predeterminado para archivos nuevos",
            Fr => "Valeur par défaut des nouveaux fichiers",
            De => "Standard für neue Dateien",
            Zh => "新文件默认",
        },
        Msg::WebCfgAudioModeCopy => match lang {
            En => "Copy the source tracks",
            It => "Copia le tracce sorgente",
            Es => "Copiar las pistas de origen",
            Fr => "Copier les pistes source",
            De => "Quellspuren kopieren",
            Zh => "复制源轨道",
        },
        Msg::WebCfgAudioModeOpus => match lang {
            En => "Convert to Opus",
            It => "Converti in Opus",
            Es => "Convertir a Opus",
            Fr => "Convertir en Opus",
            De => "In Opus konvertieren",
            Zh => "转换为 Opus",
        },
        Msg::WebSettingsNote => match lang {
            En => {
                "Encoder, quality and output changes apply to waiting jobs; track defaults apply to newly added files."
            }
            It => {
                "Le modifiche a codificatore, qualità e output valgono per i lavori in attesa; le tracce predefinite solo per i nuovi file."
            }
            Es => {
                "Los cambios de codificador, calidad y salida se aplican a los trabajos en espera; las pistas predeterminadas solo a archivos nuevos."
            }
            Fr => {
                "Les changements d'encodeur, de qualité et de sortie s'appliquent aux tâches en attente ; les pistes par défaut aux nouveaux fichiers."
            }
            De => {
                "Encoder-, Qualitäts- und Ausgabeänderungen gelten für wartende Aufträge; Spurvorgaben nur für neu hinzugefügte Dateien."
            }
            Zh => "编码器、质量和输出更改适用于等待中的任务；轨道默认值仅适用于新添加的文件。",
        },
        Msg::WebDaemonNote => match lang {
            En => {
                "Host access settings can only be changed from a browser running on the daemon host."
            }
            It => {
                "Le impostazioni di accesso all'host si modificano solo da un browser in esecuzione sull'host del daemon."
            }
            Es => {
                "Los ajustes de acceso al host solo pueden cambiarse desde un navegador ejecutado en el host del daemon."
            }
            Fr => {
                "Les paramètres d'accès à l'hôte ne sont modifiables que depuis un navigateur exécuté sur l'hôte du daemon."
            }
            De => {
                "Host-Zugriffseinstellungen können nur in einem Browser auf dem Daemon-Host geändert werden."
            }
            Zh => "主机访问设置只能从守护进程主机上运行的浏览器更改。",
        },
        Msg::WebLocalOnlyNote => match lang {
            En => "This setting is read-only for remote browsers.",
            It => "Questa impostazione è di sola lettura per i browser remoti.",
            Es => "Este ajuste es de solo lectura para navegadores remotos.",
            Fr => "Ce paramètre est en lecture seule pour les navigateurs distants.",
            De => "Diese Einstellung ist für entfernte Browser schreibgeschützt.",
            Zh => "远程浏览器只能读取此设置。",
        },
        Msg::WebRestartRequired => match lang {
            En => "Requires a daemon restart.",
            It => "Richiede il riavvio del daemon.",
            Es => "Requiere reiniciar el daemon.",
            Fr => "Nécessite le redémarrage du daemon.",
            De => "Erfordert einen Neustart des Daemons.",
            Zh => "需要重启守护进程。",
        },
        Msg::WebTokenHint => match lang {
            En => {
                "Leave blank to keep the current token; enter at least 32 characters to replace it."
            }
            It => {
                "Lascia vuoto per mantenere il token attuale; inserisci almeno 32 caratteri per sostituirlo."
            }
            Es => {
                "Déjalo vacío para conservar el token actual; introduce al menos 32 caracteres para reemplazarlo."
            }
            Fr => {
                "Laissez vide pour conserver le jeton actuel ; saisissez au moins 32 caractères pour le remplacer."
            }
            De => {
                "Leer lassen, um das aktuelle Token beizubehalten; zum Ersetzen mindestens 32 Zeichen eingeben."
            }
            Zh => "留空以保留当前令牌；输入至少 32 个字符可替换令牌。",
        },
        Msg::WebDismissSummary => match lang {
            En => "Dismiss summary",
            It => "Chiudi il riepilogo",
            Es => "Descartar el resumen",
            Fr => "Fermer le récapitulatif",
            De => "Zusammenfassung schließen",
            Zh => "关闭摘要",
        },
        Msg::WebDismiss => match lang {
            En => "Dismiss",
            It => "Chiudi",
            Es => "Cerrar",
            Fr => "Fermer",
            De => "Schließen",
            Zh => "关闭",
        },
        Msg::WebScanning => match lang {
            En => "Scanning…",
            It => "Scansione…",
            Es => "Escaneando…",
            Fr => "Analyse…",
            De => "Durchsuchen…",
            Zh => "扫描中…",
        },
        Msg::WebSaveSettings => match lang {
            En => "Save settings",
            It => "Salva impostazioni",
            Es => "Guardar ajustes",
            Fr => "Enregistrer les réglages",
            De => "Einstellungen speichern",
            Zh => "保存设置",
        },
        Msg::WebBackToQueue => match lang {
            En => "Back to queue",
            It => "Torna alla coda",
            Es => "Volver a la cola",
            Fr => "Retour à la file",
            De => "Zurück zur Warteschlange",
            Zh => "返回队列",
        },
        Msg::WebConfirmTracks => match lang {
            En => "Confirm tracks",
            It => "Conferma tracce",
            Es => "Confirmar pistas",
            Fr => "Confirmer les pistes",
            De => "Spuren bestätigen",
            Zh => "确认音轨",
        },
        Msg::WebSessionTotals => match lang {
            En => "Session totals",
            It => "Totali sessione",
            Es => "Totales de la sesión",
            Fr => "Totaux de la session",
            De => "Sitzungssummen",
            Zh => "会话总计",
        },
        Msg::WebApplyRemainingHint => match lang {
            En => "Matches tracks by order — best for files with the same track layout.",
            It => "Abbina le tracce in ordine — ideale per file con lo stesso schema di tracce.",
            Es => {
                "Empareja las pistas por orden — ideal para archivos con la misma disposición de pistas."
            }
            Fr => "Associe les pistes dans l'ordre — idéal pour des fichiers de même structure.",
            De => "Ordnet Spuren nach Reihenfolge zu — am besten bei gleichem Spurenaufbau.",
            Zh => "按顺序匹配音轨——最适合轨道结构相同的文件。",
        },
        Msg::WebSaving => match lang {
            En => "Saving…",
            It => "Salvataggio…",
            Es => "Guardando…",
            Fr => "Enregistrement…",
            De => "Wird gespeichert…",
            Zh => "保存中…",
        },
        Msg::WebFinishedWithErrors => match lang {
            En => "Finished with errors.",
            It => "Terminato con errori.",
            Es => "Finalizado con errores.",
            Fr => "Terminé avec des erreurs.",
            De => "Mit Fehlern beendet.",
            Zh => "已完成，但出现错误。",
        },
        Msg::WebConversionStopped => match lang {
            En => "Conversion stopped.",
            It => "Conversione interrotta.",
            Es => "Conversión detenida.",
            Fr => "Conversion interrompue.",
            De => "Konvertierung abgebrochen.",
            Zh => "转换已停止。",
        },
        Msg::WebLoading => match lang {
            En => "Loading…",
            It => "Caricamento…",
            Es => "Cargando…",
            Fr => "Chargement…",
            De => "Wird geladen…",
            Zh => "加载中…",
        },
        Msg::WebBrowse => match lang {
            En => "Browse…",
            It => "Sfoglia…",
            Es => "Examinar…",
            Fr => "Parcourir…",
            De => "Durchsuchen…",
            Zh => "浏览…",
        },
        Msg::WebDeleteSourceWarning => match lang {
            En => {
                "Permanent: after video quality passes, the source is deleted. Audio, subtitles and metadata are not quality-checked."
            }
            It => {
                "Permanente: superato il controllo qualità video, il sorgente viene eliminato. Audio, sottotitoli e metadati non vengono verificati."
            }
            Es => {
                "Permanente: tras superar la calidad de vídeo, se elimina el origen. Audio, subtítulos y metadatos no se verifican."
            }
            Fr => {
                "Permanent : après validation de la qualité vidéo, la source est supprimée. Audio, sous-titres et métadonnées ne sont pas vérifiés."
            }
            De => {
                "Dauerhaft: Nach bestandener Videoqualitätsprüfung wird die Quelle gelöscht. Audio, Untertitel und Metadaten werden nicht geprüft."
            }
            Zh => "永久操作：视频质量检查通过后将删除源文件。音频、字幕和元数据不会进行质量检查。",
        },
        Msg::TerminalTooSmall => match lang {
            En => "Terminal too small. Resize to at least 60 × 21. Press q to quit.",
            It => "Terminale troppo piccolo. Ridimensionalo ad almeno 60 × 21. Premi q per uscire.",
            Es => "Terminal demasiado pequeño. Ajústalo al menos a 60 × 21. Pulsa q para salir.",
            Fr => {
                "Terminal trop petit. Redimensionnez-le à au moins 60 × 21. Appuyez sur q pour quitter."
            }
            De => "Terminal zu klein. Auf mindestens 60 × 21 vergrößern. q zum Beenden drücken.",
            Zh => "终端太小。请调整到至少 60 × 21。按 q 退出。",
        },
    }
}

/// The web UI's string table: the JSON key each element and script uses, and
/// the [`Msg`] it resolves to.
///
/// Only this subset is served to the browser, not the TUI's several hundred
/// other keys. Keys reused from the TUI point at the existing [`Msg`] rather
/// than a near-duplicate.
pub const WEB_KEYS: &[(&str, Msg)] = &[
    ("add_file", Msg::WebAddFile),
    ("add_folder", Msg::WebAddFolder),
    ("add_disc", Msg::WebAddDisc),
    ("add_folder_recursive", Msg::WebAddFolderRecursive),
    ("added_files", Msg::WebAddedFiles),
    ("already_opus_copied", Msg::WebAlreadyOpusCopied),
    ("already_queued", Msg::WebAlreadyQueued),
    ("apply_remaining", Msg::WebApplyRemaining),
    ("badge_analyzing", Msg::StatusAnalyzing),
    ("badge_done", Msg::StatusDone),
    ("badge_error", Msg::Error),
    ("badge_low_vmaf", Msg::WebLowVmaf),
    ("badge_pending", Msg::Waiting),
    ("badge_ready", Msg::StatusReady),
    ("badge_skipped", Msg::Skipped),
    ("badge_verifying", Msg::StatusVerifying),
    ("badge_vmaf_failed", Msg::WebVmafFailed),
    ("cancel", Msg::Cancel),
    ("disc_chapters", Msg::DiscChapters),
    ("disc_drive_empty", Msg::DiscDriveEmpty),
    ("disc_no_drive", Msg::DiscNoDrive),
    ("disc_no_titles", Msg::DiscNoTitles),
    ("disc_open_folder", Msg::DiscOpenFolder),
    ("disc_scan_this_folder", Msg::DiscScanThisFolder),
    ("disc_select_folder", Msg::DiscSelectFolder),
    ("disc_rip", Msg::DiscRipAction),
    ("disc_scanning", Msg::DiscScanning),
    ("disc_select_drive", Msg::DiscSelectDrive),
    ("disc_select_titles", Msg::DiscSelectTitles),
    ("disc_title", Msg::HomeRipDisc),
    ("selected", Msg::SelectedWord),
    ("status_ripping", Msg::StatusRipping),
    ("cancel_encoding", Msg::CancelEncodingTitle),
    ("cancel_encoding_prompt", Msg::CancelEncodingPrompt),
    ("cancelling", Msg::WebCancelling),
    ("cfg_audio_default", Msg::WebCfgAudioDefault),
    ("cfg_audio_languages", Msg::CfgAudioLanguages),
    ("cfg_audio_mode_copy", Msg::WebCfgAudioModeCopy),
    ("cfg_audio_mode_opus", Msg::WebCfgAudioModeOpus),
    ("cfg_delete_source", Msg::CfgDeleteSource),
    ("cfg_daemon_auth_token", Msg::CfgDaemonAuthToken),
    ("cfg_daemon_autostart", Msg::CfgDaemonAutostart),
    ("cfg_daemon_bind_address", Msg::CfgDaemonBindAddress),
    ("cfg_daemon_browse_root", Msg::CfgDaemonBrowseRoot),
    ("cfg_daemon_enabled", Msg::CfgDaemonEnabled),
    ("cfg_daemon_port", Msg::CfgDaemonPort),
    ("cfg_encoder", Msg::EncoderLabel),
    ("cfg_film_grain", Msg::CfgFilmGrain),
    ("cfg_language", Msg::CfgLanguage),
    ("cfg_makemkvcon_path", Msg::CfgMakemkvconPath),
    ("cfg_nvenc_preset", Msg::CfgNvencPreset),
    ("cfg_opus_bitrate", Msg::OpusBitratePerChannel),
    ("cfg_output_container", Msg::CfgOutputContainer),
    ("cfg_output_directory", Msg::WebCfgOutputDirectory),
    ("cfg_output_suffix", Msg::CfgOutputSuffix),
    ("cfg_quality_preset", Msg::CfgQualityPreset),
    ("cfg_same_directory", Msg::CfgSameDirectory),
    ("cfg_select_all_fallback", Msg::WebCfgSelectAllFallback),
    ("cfg_skip_already_opus", Msg::SkipAlreadyOpus),
    ("cfg_staging_directory", Msg::CfgStagingDirectory),
    ("cfg_subtitle_languages", Msg::CfgSubtitleLanguages),
    ("cfg_svt_preset", Msg::CfgSvtPreset),
    ("cfg_vmaf_enabled", Msg::CfgVmafEnabled),
    ("cfg_vmaf_threshold", Msg::CfgVmafThreshold),
    ("clear_all", Msg::WebClearAll),
    ("clear_finished", Msg::WebClearFinished),
    ("col_file", Msg::FileLabel),
    ("col_saved", Msg::WebSaved),
    ("col_size", Msg::WebSize),
    ("col_source", Msg::SourceLabel),
    ("col_status", Msg::Status),
    ("current_file", Msg::WebCurrentFile),
    ("daemon_note", Msg::WebDaemonNote),
    ("local_only_note", Msg::WebLocalOnlyNote),
    ("restart_required", Msg::WebRestartRequired),
    ("token_hint", Msg::WebTokenHint),
    ("delete_source_warning", Msg::WebDeleteSourceWarning),
    ("discard_changes", Msg::DiscardConfigTitle),
    ("dismiss", Msg::WebDismiss),
    ("dolby_vision", Msg::WebDolbyVision),
    ("dv_hdr10", Msg::DvOptionHdr10),
    ("dv_keep", Msg::DvOptionKeep),
    ("dv_profile", Msg::WebDvProfile),
    ("dv_remux_hint", Msg::WebDvRemuxHint),
    ("dv_requires_svt", Msg::DvRequiresSvt),
    ("dv_source_hint", Msg::WebDvSourceHint),
    ("eta", Msg::Eta),
    ("group_audio", Msg::WebGroupAudio),
    ("group_daemon", Msg::WebGroupDaemon),
    ("group_disc", Msg::WebGroupDisc),
    ("group_general", Msg::WebGroupGeneral),
    ("group_output", Msg::WebGroupOutput),
    ("group_performance", Msg::WebGroupPerformance),
    ("group_quality", Msg::WebGroupQuality),
    ("group_rate_factors", Msg::WebGroupRateFactors),
    ("group_tracks", Msg::CfgGroupTracks),
    ("heading_audio", Msg::AudioTracks),
    ("heading_subtitles", Msg::SubtitleTracks),
    ("hidden_files", Msg::WebHiddenFiles),
    ("idle_nothing", Msg::WebIdleNothing),
    ("kind_file", Msg::WebKindFile),
    ("kind_folder", Msg::WebKindFolder),
    ("kind_not_selectable", Msg::WebKindNotSelectable),
    ("kind_symlink", Msg::WebKindSymlink),
    ("kind_video", Msg::WebKindVideoFile),
    ("move_up", Msg::MoveUp),
    ("no_audio_tracks", Msg::WebNoAudioTracks),
    ("no_subtitle_tracks", Msg::WebNoSubtitleTracks),
    ("nothing_added", Msg::WebNothingAdded),
    ("offline", Msg::WebOffline),
    ("options", Msg::WebOptions),
    ("overall_progress", Msg::WebOverallProgress),
    ("parent_directory", Msg::WebParentDirectory),
    ("qp_custom", Msg::QpCustom),
    ("qp_high", Msg::QpHigh),
    ("qp_low", Msg::QpLow),
    ("qp_medium", Msg::QpMedium),
    ("queue_empty", Msg::WebQueueEmpty),
    ("remove_from_queue", Msg::WebRemoveFromQueue),
    ("removed_finished", Msg::WebRemovedFinished),
    ("remux_hint", Msg::WebRemuxHint),
    ("remux_only", Msg::RemuxOnly),
    ("rf_full_hd", Msg::CfgRfFullHd),
    ("rf_full_hd_dv", Msg::CfgRfFullHdDv),
    ("rf_full_hd_hdr", Msg::CfgRfFullHdHdr),
    ("rf_hd", Msg::CfgRfHd),
    ("rf_sd", Msg::CfgRfSd),
    ("rf_uhd", Msg::CfgRfUhd),
    ("rf_uhd_dv", Msg::CfgRfUhdDv),
    ("rf_uhd_hdr", Msg::CfgRfUhdHdr),
    ("scanning", Msg::WebScanning),
    ("saved_exclaim", Msg::SavedExclaim),
    ("select_all", Msg::WebSelectAll),
    ("select_folder", Msg::SelectFolder),
    ("select_folder_recursive", Msg::WebSelectFolderRecursive),
    ("select_this_folder", Msg::SelectThisFolder),
    ("select_video_file", Msg::SelectVideoFile),
    ("settings_note", Msg::WebSettingsNote),
    ("stat_in_queue", Msg::WebStatInQueue),
    ("stat_space_saved", Msg::TotalSpaceSaved),
    ("status_encoding", Msg::Encoding),
    ("status_idle", Msg::WebIdle),
    ("summary_complete", Msg::ConversionComplete),
    ("summary_converted", Msg::Converted),
    ("summary_dismiss", Msg::WebDismissSummary),
    ("summary_errors", Msg::Errors),
    ("summary_time", Msg::TotalTime),
    ("tab_queue", Msg::WebTabQueue),
    ("tab_settings", Msg::Settings),
    ("tag_remux", Msg::WebTagRemux),
    ("tag_source_deleted", Msg::SourceDeletedTag),
    ("track_copy", Msg::CopyTracks),
    ("track_exclude", Msg::WebTrackExclude),
    ("track_opus", Msg::ToOpus),
    ("tracks_close", Msg::WebCloseTracks),
    ("tracks_applied", Msg::WebTracksApplied),
    ("tracks_hint", Msg::WebTracksHint),
    ("tracks_locked", Msg::WebTracksLocked),
    ("tracks_title", Msg::WebTracksTitle),
    ("tracks_updated", Msg::WebTracksUpdated),
    ("unauthorized", Msg::WebUnauthorized),
    ("save_settings", Msg::WebSaveSettings),
    ("back_to_queue", Msg::WebBackToQueue),
    ("confirm_tracks", Msg::WebConfirmTracks),
    ("session_totals", Msg::WebSessionTotals),
    ("apply_remaining_hint", Msg::WebApplyRemainingHint),
    ("autostart_unsupported", Msg::DaemonServiceUnsupported),
    ("saving", Msg::WebSaving),
    ("browse", Msg::WebBrowse),
    ("loading", Msg::WebLoading),
    ("summary_failed", Msg::WebFinishedWithErrors),
    ("summary_stopped", Msg::WebConversionStopped),
    ("verifying_vmaf", Msg::StatusVerifying),
];

#[cfg(test)]
mod tests {
    use super::{Language, WEB_KEYS, t};
    use std::collections::HashSet;

    /// Web keys are looked up by name, so each must be unique.
    #[test]
    fn web_keys_are_unique() {
        let mut seen: HashSet<&str> = HashSet::new();
        for (key, _) in WEB_KEYS {
            assert!(seen.insert(key), "duplicate web key: {key}");
        }
    }

    /// Every served key resolves to a non-empty string in every language, and
    /// to something other than the English text in at least one of them.
    #[test]
    fn every_web_key_is_translated() {
        // The rate-factor tier labels ("RF 1080p HDR") are technical
        // abbreviations, written the same way in every locale.
        const INVARIANT: &[&str] = &[
            "rf_full_hd",
            "rf_full_hd_dv",
            "rf_full_hd_hdr",
            "rf_hd",
            "rf_sd",
            "rf_uhd",
            "rf_uhd_dv",
            "rf_uhd_hdr",
        ];

        for (key, msg) in WEB_KEYS {
            let resolved: Vec<&str> = Language::ALL.iter().map(|&l| t(l, *msg)).collect();
            for (lang, text) in Language::ALL.iter().zip(&resolved) {
                assert!(
                    !text.trim().is_empty(),
                    "web key {key} is empty in {lang:?}"
                );
            }
            if !INVARIANT.contains(key) {
                let distinct: HashSet<&&str> = resolved.iter().collect();
                assert!(
                    distinct.len() > 1,
                    "web key {key} resolves to {:?} in all six languages; \
                     either translate it or list it as invariant",
                    resolved[0]
                );
            }
        }
    }

    /// Every translation keeps the placeholders the browser substitutes.
    #[test]
    fn placeholders_survive_every_translation() {
        for (key, msg) in WEB_KEYS {
            let english = t(Language::English, *msg);
            for name in ["{n}", "{profile}"] {
                if english.contains(name) {
                    for &lang in &Language::ALL {
                        assert!(
                            t(lang, *msg).contains(name),
                            "web key {key} loses {name} in {lang:?}"
                        );
                    }
                }
            }
        }
    }
}
