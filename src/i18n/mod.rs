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
    AddToQueue,
    AlreadyOpus,
    SubtitleNotIncluded,
    OpusUnavailable,
    AudioMode,
    OpusBitratePerChannel,
    SkipAlreadyOpus,
    OpenFolderAction,
    SelectThisFolder,
    SelectCurrentFolder,
    SwitchFile,
    Cancelling,
    ShuttingDown,

    // ── Home ─────────────────────────────────────────────────────────────────
    MenuTitle,
    AppTitle,
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
    DiscChapterCount,
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
    SelectedCount,
    NoVideoFiles,
    ScanningFiles,
    FolderScanFailed,
    NonUtf8Path,
    DriveDiscoveryStopped,
    DiscRunStopped,
    EncodingStopped,

    // ── File confirm ─────────────────────────────────────────────────────────
    ConfirmSelection,
    Files,
    FilesSelectedCount,

    // ── Track config ─────────────────────────────────────────────────────────
    FileLabel,
    ResolutionLabel,
    TypeLabel,
    ModeLabel,
    OutputFileLabel,
    RemuxOnly,
    EncodeVideo,
    VideoInfo,
    ScrollDetails,
    DiscOperationRunning,
    DetectingEncoder,
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
    DvConvertedTag,
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
    KeepDvConverted,
    KeepAudioTranscoded,
    KeepSubtitleChanged,
    KeepDvProfile7,
    KeepChromaOrBitDepth,
    KeepStreamsUnchecked,
    KeepExtraStreams,
    KeepAttachments,
    KeepCancelled,
    KeepOutputChanged,
    KeepFlushFailed,
    KeepSymlink,
    KeepSourceChanged,
    KeepDeleteFailed,
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
    ExitAppUnsavedPrompt,
    ExitAppRipsPrompt,
    AbandonTrackConfigTitle,
    AbandonTrackConfigPrompt,
    DiscardConfigTitle,
    DiscardConfigPrompt,
    CancelAnalysisTitle,
    CancelAnalysisPrompt,
    FinishResetPrompt,
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
    InvalidContainer,
    InvalidAuthToken,
    OutputDirectoryMissing,
    OutputDirectoryInvalid,
    BrowseRootRequired,
    QueueUnreadable,
    BrowseRootInvalid,
    StagingDirectoryInvalid,
    MakemkvconInvalid,
    TokenTooShort,
    OutputDirectoryOutsideBrowseRoot,
    SettingsRestoreFailed,
    BrowseRootExcludesJobs,
    QueuedRipsNeedOutputDirectory,
    QueuedRipsPinStagingDirectory,
    ThresholdTooLowToDelete,
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
    CfgQsvQuality,
    CfgAmfQuality,
    EncoderSvtAv1,
    VmafMinScore,
    WebPrimaryNav,
    WebRequestFailed,
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
    CfgDaemonAllowInsecureLan,
    CfgDaemonBehindProxy,

    // ── Daemon mode ──────────────────────────────────────────────────────────
    DaemonDisabledError,
    DaemonListening,
    DaemonPublicHttp,
    DaemonPublicHttpRefused,
    DaemonTokenGenerated,
    DaemonTokenGeneratedSeeStatus,
    EncoderUnavailable,
    ConfigLoadFailed,
    ConfigUnreadableRefused,
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
    DaemonServiceStillStarting,
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
    WebReasonRestartInterrupted,
    WebSelectFolderRecursive,
    WebHiddenFiles,
    WebParentDirectory,
    WebKindFolder,
    WebKindVideoFile,
    WebKindFile,
    WebKindSymlink,
    WebKindNotSelectable,
    WebKindDiscImage,
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
    WebDvSourceHintPlain,
    WebDvHint,
    WebDvProfile,
    WebCancelling,
    WebRemovedFinished,
    WebAddedFiles,
    WebAlreadyQueued,
    WebNothingAdded,
    WebSkippedFiles,
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
    WebSessionSummary,
    WebSessionSummaryCancelled,
    WebApplyRemainingHint,
    WebSaving,
    WebBrowse,
    WebLoading,
    WebFinishedWithErrors,
    WebConversionStopped,
    WebRemoveRipPrompt,
    WebClearFinishedRipPrompt,
    WebDurationHoursMinutes,
    WebDurationMinutesSeconds,
    WebDurationSeconds,
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
        Msg::AddToQueue => match lang {
            En => "Add to queue",
            It => "Aggiungi alla coda",
            Es => "Añadir a la cola",
            Fr => "Ajouter à la file",
            De => "Zur Warteschlange hinzufügen",
            Zh => "添加到队列",
        },
        Msg::AlreadyOpus => match lang {
            En => "already Opus",
            It => "già Opus",
            Es => "ya es Opus",
            Fr => "déjà Opus",
            De => "bereits Opus",
            Zh => "已是 Opus",
        },
        Msg::SubtitleNotIncluded => match lang {
            En => "not included: unsupported by the container",
            It => "non incluso: il contenitore non lo supporta",
            Es => "no incluido: el contenedor no lo admite",
            Fr => "non inclus : le conteneur ne le prend pas en charge",
            De => "nicht enthalten: vom Container nicht unterstützt",
            Zh => "不包含：容器不支持",
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
        Msg::SelectCurrentFolder => match lang {
            En => "Select current folder",
            It => "Seleziona la cartella corrente",
            Es => "Seleccionar la carpeta actual",
            Fr => "Sélectionner le dossier actuel",
            De => "Aktuellen Ordner wählen",
            Zh => "选择当前文件夹",
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
        Msg::AppTitle => match lang {
            En => "AV1 Video Converter",
            It => "Convertitore video AV1",
            Es => "Conversor de video AV1",
            Fr => "Convertisseur vidéo AV1",
            De => "AV1-Videokonverter",
            Zh => "AV1 视频转换器",
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
            Zh => "翻录 DVD / 蓝光",
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
            De => "Markieren Sie vor dem Auslesen einen Titel mit der Leertaste.",
            Zh => "开始翻录前请用空格键标记标题。",
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
            Fr => "Analyser l'image disque",
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
            Zh => "翻录",
        },
        Msg::WebAddDisc => match lang {
            En | De => "+ Disc",
            It | Es => "+ Disco",
            Fr => "+ Disque",
            Zh => "+ 光盘",
        },
        Msg::DiscChapterCount => match lang {
            En => "{n} chapters",
            It => "{n} capitoli",
            Es => "{n} capítulos",
            Fr => "{n} chapitres",
            De => "{n} Kapitel",
            Zh => "{n} 个章节",
        },
        Msg::StatusRipping => match lang {
            En => "Ripping",
            It => "Estrazione",
            Es => "Extrayendo",
            Fr => "Extraction",
            De => "Wird ausgelesen",
            Zh => "翻录中",
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
                "Imposta una cartella di output nelle impostazioni prima di estrarre: la codifica non può essere scritta nella cartella di staging."
            }
            Es => {
                "Define una carpeta de salida en los ajustes antes de extraer: la codificación no puede escribirse en la carpeta temporal."
            }
            Fr => {
                "Choisissez un dossier de sortie dans les paramètres avant d'extraire : l'encodage ne peut pas être écrit dans le dossier temporaire."
            }
            De => {
                "Legen Sie vor dem Auslesen einen Ausgabeordner in den Einstellungen fest: Die Kodierung kann nicht in den Zwischenordner geschrieben werden."
            }
            Zh => "翻录前请在设置中指定输出目录：编码结果不能写入暂存目录。",
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
            Es => "Carpeta actual",
            Fr => "Dossier actuel",
            De => "Aktueller Ordner",
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
        Msg::SelectedCount => match lang {
            En => "{n} selected",
            It => "{n} selezionati",
            Es => "{n} seleccionados",
            Fr => "{n} sélectionnés",
            De => "{n} ausgewählt",
            Zh => "已选择 {n} 个",
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
        Msg::NonUtf8Path => match lang {
            En => "File path contains non-UTF-8 characters",
            It => "Il percorso del file contiene caratteri non UTF-8",
            Es => "La ruta del archivo contiene caracteres no UTF-8",
            Fr => "Le chemin du fichier contient des caractères non UTF-8",
            De => "Der Dateipfad enthält Nicht-UTF-8-Zeichen",
            Zh => "文件路径包含非 UTF-8 字符",
        },
        Msg::DriveDiscoveryStopped => match lang {
            En => "drive discovery stopped unexpectedly",
            It => "la ricerca delle unità si è interrotta inaspettatamente",
            Es => "la detección de unidades se detuvo inesperadamente",
            Fr => "la détection des lecteurs s'est arrêtée de façon inattendue",
            De => "die Laufwerkssuche wurde unerwartet beendet",
            Zh => "光驱检测意外停止",
        },
        Msg::DiscRunStopped => match lang {
            En => "the run stopped unexpectedly",
            It => "l'operazione si è interrotta inaspettatamente",
            Es => "la operación se detuvo inesperadamente",
            Fr => "l'opération s'est arrêtée de façon inattendue",
            De => "der Vorgang wurde unerwartet beendet",
            Zh => "操作意外停止",
        },
        Msg::EncodingStopped => match lang {
            En => "Encoding stopped unexpectedly",
            It => "La codifica si è interrotta inaspettatamente",
            Es => "La codificación se detuvo inesperadamente",
            Fr => "L'encodage s'est arrêté de façon inattendue",
            De => "Die Kodierung wurde unerwartet beendet",
            Zh => "编码意外停止",
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
        Msg::FilesSelectedCount => match lang {
            En => "{n} files selected",
            It => "{n} file selezionati",
            Es => "{n} archivos seleccionados",
            Fr => "{n} fichiers sélectionnés",
            De => "{n} Dateien ausgewählt",
            Zh => "已选择 {n} 个文件",
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
        Msg::DvConvertedTag => match lang {
            En => "to HDR10",
            It => "in HDR10",
            Es => "a HDR10",
            Fr => "vers HDR10",
            De => "zu HDR10",
            Zh => "转为 HDR10",
        },
        Msg::UseRecommended => match lang {
            En => "Use recommended",
            It => "Usa l'opzione consigliata",
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
        Msg::DiscOperationRunning => match lang {
            En => "A disc operation is already running",
            It => "Un'operazione su disco è già in corso",
            Es => "Ya hay una operación de disco en curso",
            Fr => "Une opération sur disque est déjà en cours",
            De => "Es läuft bereits ein Disc-Vorgang",
            Zh => "已有光盘操作正在进行",
        },
        Msg::DetectingEncoder => match lang {
            En => "Detecting the AV1 encoder…",
            It => "Rilevamento del codificatore AV1…",
            Es => "Detectando el codificador AV1…",
            Fr => "Détection de l'encodeur AV1…",
            De => "AV1-Encoder wird erkannt…",
            Zh => "正在检测 AV1 编码器…",
        },
        Msg::ScrollDetails => match lang {
            En => "Scroll details",
            It => "Scorri dettagli",
            Es => "Desplazar detalles",
            Fr => "Faire défiler les détails",
            De => "Details scrollen",
            Zh => "滚动详情",
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
            It => "Sorgente",
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
            It => "File sorgente eliminato",
            Es => "Archivo de origen eliminado",
            Fr => "Fichier source supprimé",
            De => "Quelldatei gelöscht",
            Zh => "已删除源文件",
        },
        Msg::SourceKept => match lang {
            En => "Source kept",
            It => "Sorgente mantenuta",
            Es => "Origen conservado",
            Fr => "Source conservée",
            De => "Quelle behalten",
            Zh => "已保留源文件",
        },
        Msg::KeepDvConverted => match lang {
            En => "Dolby Vision was converted to HDR10",
            It => "Dolby Vision è stato convertito in HDR10",
            Es => "Dolby Vision se convirtió a HDR10",
            Fr => "Dolby Vision a été converti en HDR10",
            De => "Dolby Vision wurde in HDR10 umgewandelt",
            Zh => "Dolby Vision 已转换为 HDR10",
        },
        Msg::KeepAudioTranscoded => match lang {
            En => "audio was transcoded and VMAF does not verify it",
            It => "l'audio è stato ricodificato e VMAF non lo verifica",
            Es => "el audio se recodificó y VMAF no lo verifica",
            Fr => "l'audio a été réencodé et VMAF ne le vérifie pas",
            De => "Audio wurde neu kodiert und VMAF prüft es nicht",
            Zh => "音频已转码，VMAF 无法验证",
        },
        Msg::KeepSubtitleChanged => match lang {
            En => "a selected subtitle track was converted or left out",
            It => "una traccia di sottotitoli selezionata è stata convertita o esclusa",
            Es => "una pista de subtítulos seleccionada se convirtió o se omitió",
            Fr => "une piste de sous-titres sélectionnée a été convertie ou omise",
            De => "eine gewählte Untertitelspur wurde umgewandelt oder ausgelassen",
            Zh => "所选字幕轨道被转换或丢弃",
        },
        Msg::KeepDvProfile7 => match lang {
            En => "the Dolby Vision profile 7 enhancement layer is not carried over",
            It => "il livello di miglioramento Dolby Vision profilo 7 non viene trasferito",
            Es => "la capa de mejora de Dolby Vision perfil 7 no se conserva",
            Fr => "la couche d'amélioration Dolby Vision profil 7 n'est pas conservée",
            De => "die Dolby-Vision-Profil-7-Erweiterungsebene wird nicht übernommen",
            Zh => "Dolby Vision profile 7 增强层未保留",
        },
        Msg::KeepChromaOrBitDepth => match lang {
            En => "chroma or bit depth may be reduced by the 4:2:0 10-bit encode",
            It => "croma o profondità di bit possono essere ridotti dalla codifica 4:2:0 a 10 bit",
            Es => {
                "el croma o la profundidad de bits pueden reducirse con la codificación 4:2:0 de 10 bits"
            }
            Fr => {
                "la chroma ou la profondeur de bits peut être réduite par l'encodage 4:2:0 10 bits"
            }
            De => "Chroma oder Bittiefe können durch die 4:2:0-10-Bit-Kodierung verringert werden",
            Zh => "4:2:0 10 位编码可能降低色度或位深",
        },
        Msg::KeepStreamsUnchecked => match lang {
            En => "the source streams could not be checked",
            It => "i flussi della sorgente non sono stati verificati",
            Es => "no se pudieron comprobar las pistas del origen",
            Fr => "les flux de la source n'ont pas pu être vérifiés",
            De => "die Quellstreams konnten nicht geprüft werden",
            Zh => "无法检查源文件的流",
        },
        Msg::KeepExtraStreams => match lang {
            En => "the source has video or data streams the output does not carry",
            It => "la sorgente ha flussi video o dati che l'uscita non contiene",
            Es => "el origen tiene pistas de vídeo o datos que la salida no incluye",
            Fr => "la source a des flux vidéo ou de données absents de la sortie",
            De => "die Quelle hat Video- oder Datenstreams, die die Ausgabe nicht enthält",
            Zh => "源文件含有输出未包含的视频或数据流",
        },
        Msg::KeepAttachments => match lang {
            En => "cover art or attachments are not carried into the output",
            It => "copertine o allegati non vengono trasferiti nell'uscita",
            Es => "las carátulas o los adjuntos no se conservan en la salida",
            Fr => "les pochettes ou pièces jointes ne sont pas conservées dans la sortie",
            De => "Cover oder Anhänge werden nicht in die Ausgabe übernommen",
            Zh => "封面或附件未写入输出",
        },
        Msg::KeepCancelled => match lang {
            En => "cancellation was requested",
            It => "è stato richiesto l'annullamento",
            Es => "se solicitó la cancelación",
            Fr => "l'annulation a été demandée",
            De => "Abbruch wurde angefordert",
            Zh => "已请求取消",
        },
        Msg::KeepOutputChanged => match lang {
            En => "the encoded output changed before deletion",
            It => "l'uscita codificata è cambiata prima dell'eliminazione",
            Es => "la salida codificada cambió antes del borrado",
            Fr => "la sortie encodée a changé avant la suppression",
            De => "die kodierte Ausgabe hat sich vor dem Löschen geändert",
            Zh => "编码输出在删除前发生变化",
        },
        Msg::KeepFlushFailed => match lang {
            En => "the encoded output could not be flushed to disk",
            It => "l'uscita codificata non è stata scritta su disco",
            Es => "no se pudo volcar la salida codificada al disco",
            Fr => "la sortie encodée n'a pas pu être écrite sur le disque",
            De => "die kodierte Ausgabe konnte nicht auf die Festplatte geschrieben werden",
            Zh => "编码输出无法写入磁盘",
        },
        Msg::KeepSymlink => match lang {
            En => "the source is a symbolic link",
            It => "la sorgente è un collegamento simbolico",
            Es => "el origen es un enlace simbólico",
            Fr => "la source est un lien symbolique",
            De => "die Quelle ist eine symbolische Verknüpfung",
            Zh => "源文件是符号链接",
        },
        Msg::KeepSourceChanged => match lang {
            En => "the source changed while the job was running",
            It => "la sorgente è cambiata durante il lavoro",
            Es => "el origen cambió mientras se ejecutaba el trabajo",
            Fr => "la source a changé pendant le travail",
            De => "die Quelle hat sich während des Auftrags geändert",
            Zh => "任务运行期间源文件发生变化",
        },
        Msg::KeepDeleteFailed => match lang {
            En => "deleting the source failed",
            It => "l'eliminazione della sorgente non è riuscita",
            Es => "no se pudo borrar el origen",
            Fr => "la suppression de la source a échoué",
            De => "das Löschen der Quelle ist fehlgeschlagen",
            Zh => "删除源文件失败",
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
            It => "sorgente eliminata",
            Es => "origen eliminado",
            Fr => "source supprimée",
            De => "Quelle gelöscht",
            Zh => "源文件已删除",
        },
        Msg::SourceKeptTag => match lang {
            En => "source kept",
            It => "sorgente mantenuta",
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
            En => "Cancel the current encoding? Jobs waiting to encode are cancelled too.",
            It => {
                "Annullare la codifica in corso? Anche i file in attesa di codifica verranno annullati."
            }
            Es => {
                "¿Cancelar la codificación actual? Los trabajos en espera de codificación también se cancelan."
            }
            Fr => {
                "Annuler l'encodage en cours ? Les tâches en attente d'encodage sont aussi annulées."
            }
            De => "Laufende Kodierung abbrechen? Wartende Aufträge werden ebenfalls abgebrochen.",
            Zh => "取消当前编码？等待编码的任务也会被取消。",
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
            De => "Das laufende Lesen oder Auslesen der Disc abbrechen?",
            Zh => "取消当前的光盘扫描或翻录吗？",
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
        Msg::ExitAppUnsavedPrompt => match lang {
            En => "Unsaved settings will be lost. Exit anyway?",
            It => "Le impostazioni non salvate andranno perse. Uscire comunque?",
            Es => "Los ajustes sin guardar se perderán. ¿Salir de todos modos?",
            Fr => "Les paramètres non enregistrés seront perdus. Quitter quand même ?",
            De => "Ungespeicherte Einstellungen gehen verloren. Trotzdem beenden?",
            Zh => "未保存的设置将丢失。仍要退出吗？",
        },
        Msg::ExitAppRipsPrompt => match lang {
            En => "Ripped titles still in the queue will be deleted. Exit anyway?",
            It => "I titoli estratti ancora in coda verranno eliminati. Uscire comunque?",
            Es => {
                "Los títulos extraídos que siguen en la cola se eliminarán. ¿Salir de todos modos?"
            }
            Fr => "Les titres extraits encore dans la file seront supprimés. Quitter quand même ?",
            De => "Ausgelesene Titel in der Warteschlange werden gelöscht. Trotzdem beenden?",
            Zh => "队列中的已翻录标题将被删除。仍要退出吗？",
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
            It => "Annullare l'analisi dei file rimanenti?",
            Es => "¿Cancelar el análisis de los archivos restantes?",
            Fr => "Annuler l'analyse des fichiers restants ?",
            De => "Die Analyse der übrigen Dateien abbrechen?",
            Zh => "取消其余文件的分析吗？",
        },
        Msg::FinishResetPrompt => match lang {
            En => "Clear the queue and return to the home screen?",
            It => "Svuotare la coda e tornare alla schermata iniziale?",
            Es => "¿Vaciar la cola y volver a la pantalla de inicio?",
            Fr => "Vider la file et revenir à l'écran d'accueil ?",
            De => "Warteschlange leeren und zum Startbildschirm zurückkehren?",
            Zh => "清空队列并返回主屏幕？",
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
        Msg::InvalidContainer => match lang {
            En => "Container must be mkv, mp4 or webm",
            It => "Il contenitore deve essere mkv, mp4 o webm",
            Es => "El contenedor debe ser mkv, mp4 o webm",
            Fr => "Le conteneur doit être mkv, mp4 ou webm",
            De => "Der Container muss mkv, mp4 oder webm sein",
            Zh => "容器必须是 mkv、mp4 或 webm",
        },
        Msg::InvalidAuthToken => match lang {
            En => "Access token must use printable ASCII characters and no spaces",
            It => "Il token di accesso deve usare caratteri ASCII stampabili, senza spazi",
            Es => "El token de acceso debe usar caracteres ASCII imprimibles, sin espacios",
            Fr => "Le jeton d'accès doit utiliser des caractères ASCII imprimables, sans espaces",
            De => "Das Zugriffstoken darf nur druckbare ASCII-Zeichen ohne Leerzeichen enthalten",
            Zh => "访问令牌只能使用可打印的 ASCII 字符，且不能包含空格",
        },
        Msg::OutputDirectoryMissing => match lang {
            En => {
                "Saved, but the output directory does not exist: encodes and disc rips that write there will fail until it is created or changed"
            }
            It => {
                "Salvato, ma la cartella di output non esiste: le codifiche e le estrazioni dei dischi che scrivono lì falliranno finché non viene creata o cambiata"
            }
            Es => {
                "Guardado, pero la carpeta de salida no existe: las codificaciones y las extracciones de discos que escriben allí fallarán hasta que se cree o se cambie"
            }
            Fr => {
                "Enregistré, mais le dossier de sortie n'existe pas : les encodages et les extractions de disques qui y écrivent échoueront tant qu'il n'est pas créé ou modifié"
            }
            De => {
                "Gespeichert, aber der Ausgabeordner existiert nicht: Kodierungen und ausgelesene Discs, die dorthin geschrieben werden, schlagen fehl, bis er angelegt oder geändert wird"
            }
            Zh => {
                "已保存，但输出目录不存在：在创建或更改该目录之前，写入该目录的编码和光盘翻录将会失败"
            }
        },
        Msg::OutputDirectoryInvalid => match lang {
            En => "Output directory must be an existing directory",
            It => "La cartella di output deve essere una cartella esistente",
            Es => "La carpeta de salida debe ser una carpeta existente",
            Fr => "Le dossier de sortie doit être un dossier existant",
            De => "Der Ausgabeordner muss ein vorhandener Ordner sein",
            Zh => "输出目录必须是已存在的目录",
        },
        Msg::QueueUnreadable => match lang {
            En => "The saved queue could not be read; its contents were kept in {path}",
            It => "Impossibile leggere la coda salvata; il contenuto è stato conservato in {path}",
            Es => "No se pudo leer la cola guardada; su contenido se conservó en {path}",
            Fr => "Impossible de lire la file enregistrée ; son contenu a été conservé dans {path}",
            De => {
                "Die gespeicherte Warteschlange war nicht lesbar; ihr Inhalt wurde in {path} aufbewahrt"
            }
            Zh => "无法读取已保存的队列；其内容已保留在 {path}",
        },
        Msg::BrowseRootRequired => match lang {
            En => {
                "A daemon browse root (daemon.browse_root) is required when binding outside loopback"
            }
            It => {
                "Serve una cartella base daemon (daemon.browse_root) per un bind fuori dal loopback"
            }
            Es => {
                "Se necesita una carpeta base del daemon (daemon.browse_root) para un bind fuera de loopback"
            }
            Fr => {
                "Un dossier racine du daemon (daemon.browse_root) est requis pour écouter hors loopback"
            }
            De => {
                "Für einen Bind außerhalb von Loopback ist ein Daemon-Basisordner (daemon.browse_root) erforderlich"
            }
            Zh => "绑定到非回环地址时必须设置守护进程浏览根目录（daemon.browse_root）",
        },
        Msg::BrowseRootInvalid => match lang {
            En => "Daemon browse root must be an existing directory",
            It => "La cartella base daemon deve essere una cartella esistente",
            Es => "La carpeta base del daemon debe ser una carpeta existente",
            Fr => "Le dossier racine du daemon doit être un dossier existant",
            De => "Der Daemon-Basisordner muss ein vorhandener Ordner sein",
            Zh => "守护进程浏览根目录必须是已存在的目录",
        },
        Msg::StagingDirectoryInvalid => match lang {
            En => "Disc staging directory must be an existing directory",
            It => "La cartella di staging dischi deve essere una cartella esistente",
            Es => "La carpeta temporal de discos debe ser una carpeta existente",
            Fr => "Le dossier temporaire des disques doit être un dossier existant",
            De => "Der Zwischenordner für Discs muss ein vorhandener Ordner sein",
            Zh => "光盘暂存目录必须是已存在的目录",
        },
        Msg::MakemkvconInvalid => match lang {
            En => "MakeMKV path must be an existing file named makemkvcon or makemkvcon64",
            It => {
                "Il percorso di MakeMKV deve essere un file esistente chiamato makemkvcon o makemkvcon64"
            }
            Es => {
                "La ruta de MakeMKV debe ser un archivo existente llamado makemkvcon o makemkvcon64"
            }
            Fr => {
                "Le chemin de MakeMKV doit être un fichier existant nommé makemkvcon ou makemkvcon64"
            }
            De => {
                "Der MakeMKV-Pfad muss eine vorhandene Datei namens makemkvcon oder makemkvcon64 sein"
            }
            Zh => "MakeMKV 路径必须是名为 makemkvcon 或 makemkvcon64 的已存在文件",
        },
        Msg::TokenTooShort => match lang {
            En => "Access token must contain at least 32 characters",
            It => "Il token di accesso deve contenere almeno 32 caratteri",
            Es => "El token de acceso debe contener al menos 32 caracteres",
            Fr => "Le jeton d'accès doit contenir au moins 32 caractères",
            De => "Das Zugriffstoken muss mindestens 32 Zeichen enthalten",
            Zh => "访问令牌必须至少包含 32 个字符",
        },
        Msg::SettingsRestoreFailed => match lang {
            En => {
                "The settings were refused, but config.toml still holds them: the previous settings could not be written back"
            }
            It => {
                "Le impostazioni sono state rifiutate, ma config.toml le contiene ancora: non è stato possibile riscrivere quelle precedenti"
            }
            Es => {
                "La configuración se rechazó, pero config.toml aún la contiene: no se pudo volver a escribir la anterior"
            }
            Fr => {
                "Les paramètres ont été refusés, mais config.toml les contient encore : les paramètres précédents n'ont pas pu être réécrits"
            }
            De => {
                "Die Einstellungen wurden abgelehnt, stehen aber noch in config.toml: die vorherigen Einstellungen konnten nicht zurückgeschrieben werden"
            }
            Zh => "设置已被拒绝，但 config.toml 中仍保存着它们：无法写回之前的设置",
        },
        Msg::OutputDirectoryOutsideBrowseRoot => match lang {
            En => "Output directory must be an existing directory inside the daemon browse root",
            It => {
                "La cartella di output deve essere una cartella esistente dentro la cartella base daemon"
            }
            Es => {
                "La carpeta de salida debe ser una carpeta existente dentro de la carpeta base del daemon"
            }
            Fr => {
                "Le dossier de sortie doit être un dossier existant dans le dossier racine du daemon"
            }
            De => "Der Ausgabeordner muss ein vorhandener Ordner im Daemon-Basisordner sein",
            Zh => "输出目录必须是守护进程浏览根目录内的现有目录",
        },
        Msg::BrowseRootExcludesJobs => match lang {
            En => "The new browse root leaves out one or more queued jobs",
            It => "La nuova cartella base esclude uno o più lavori in coda",
            Es => "La nueva carpeta base deja fuera uno o más trabajos en cola",
            Fr => "Le nouveau dossier racine exclut une ou plusieurs tâches de la file",
            De => {
                "Der neue Basisordner schließt einen oder mehrere Aufträge in der Warteschlange aus"
            }
            Zh => "新的浏览根目录不包含一个或多个排队中的任务",
        },
        Msg::ThresholdTooLowToDelete => match lang {
            En => "A VMAF threshold of 0 proves nothing; raise it or turn off deleting the source",
            It => {
                "Una soglia VMAF di 0 non dimostra nulla: alzala o disattiva l'eliminazione dell'originale"
            }
            Es => {
                "Un umbral VMAF de 0 no demuestra nada: súbelo o desactiva el borrado del original"
            }
            Fr => {
                "Un seuil VMAF de 0 ne prouve rien : augmentez-le ou désactivez la suppression de la source"
            }
            De => {
                "Ein VMAF-Schwellenwert von 0 belegt nichts: Erhöhen Sie ihn oder schalten Sie das Löschen der Quelle aus"
            }
            Zh => "VMAF 阈值为 0 无法证明任何内容：请提高阈值或关闭删除源文件",
        },
        Msg::QueuedRipsPinStagingDirectory => match lang {
            En => "Queued disc rips are stored in the current staging directory",
            It => "Le estrazioni dei dischi in coda si trovano nella cartella di staging attuale",
            Es => "Las extracciones de discos en cola están en la carpeta de preparación actual",
            Fr => "Les extractions de disques en file sont dans le dossier de préparation actuel",
            De => "Ausgelesene Discs in der Warteschlange liegen im aktuellen Staging-Ordner",
            Zh => "队列中的光盘翻录保存在当前暂存目录中",
        },
        Msg::QueuedRipsNeedOutputDirectory => match lang {
            En => "Queued disc rips need an output directory",
            It => "Le estrazioni dei dischi in coda richiedono una cartella di output",
            Es => "Las extracciones de discos en cola necesitan una carpeta de salida",
            Fr => "Les extractions de disques en file ont besoin d'un dossier de sortie",
            De => "Ausgelesene Discs in der Warteschlange benötigen einen Ausgabeordner",
            Zh => "队列中的光盘翻录需要输出目录",
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
            It => "Elimina sorgente se VMAF OK",
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
        Msg::CfgQsvQuality => match lang {
            En => "QSV Quality",
            It => "Qualità QSV",
            Es => "Calidad QSV",
            Fr => "Qualité QSV",
            De => "QSV-Qualität",
            Zh => "QSV 质量",
        },
        Msg::CfgAmfQuality => match lang {
            En => "AMF Quality",
            It => "Qualità AMF",
            Es => "Calidad AMF",
            Fr => "Qualité AMF",
            De => "AMF-Qualität",
            Zh => "AMF 质量",
        },
        Msg::EncoderSvtAv1 => match lang {
            En | It | Es => "SVT-AV1 (software)",
            Fr => "SVT-AV1 (logiciel)",
            De => "SVT-AV1 (Software)",
            Zh => "SVT-AV1（软件）",
        },
        Msg::VmafMinScore => match lang {
            En | It | Fr => "(min {score})",
            Es => "(mín {score})",
            De => "(Min. {score})",
            Zh => "（最低 {score}）",
        },
        Msg::WebPrimaryNav => match lang {
            En => "Primary",
            It | Fr => "Principale",
            Es => "Principal",
            De => "Hauptnavigation",
            Zh => "主导航",
        },
        Msg::WebRequestFailed => match lang {
            En => "Request failed ({status})",
            It => "Richiesta non riuscita ({status})",
            Es => "La solicitud ha fallado ({status})",
            Fr => "Échec de la requête ({status})",
            De => "Anfrage fehlgeschlagen ({status})",
            Zh => "请求失败（{status}）",
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
            Es => "Salida en la misma carpeta",
            Fr => "Sortie dans le même dossier",
            De => "Ausgabe im selben Ordner",
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
            Es => "Ejecutar al iniciar sesión",
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
        Msg::CfgDaemonAllowInsecureLan => match lang {
            En => "Allow insecure LAN bind (plain HTTP)",
            It => "Consenti bind LAN non sicuro (HTTP in chiaro)",
            Es => "Permitir bind LAN inseguro (HTTP sin cifrar)",
            Fr => "Autoriser une écoute LAN non sécurisée (HTTP clair)",
            De => "Unsicheren LAN-Bind erlauben (Klartext-HTTP)",
            Zh => "允许不安全的局域网绑定（明文 HTTP）",
        },
        Msg::CfgDaemonBehindProxy => match lang {
            En => "Behind a reverse proxy (treat every web request as remote)",
            It => "Dietro un reverse proxy (tratta ogni richiesta web come remota)",
            Es => "Detrás de un proxy inverso (tratar cada petición web como remota)",
            Fr => "Derrière un reverse proxy (traiter chaque requête web comme distante)",
            De => "Hinter einem Reverse-Proxy (jede Web-Anfrage als entfernt behandeln)",
            Zh => "位于反向代理之后（将每个 Web 请求视为远程请求）",
        },
        Msg::DaemonDisabledError => match lang {
            En => {
                "Daemon mode is disabled. Enable it in Settings or set enabled = true under [daemon] in config.toml."
            }
            It => {
                "La modalità daemon è disabilitata. Abilitala nelle Impostazioni o imposta enabled = true sotto [daemon] in config.toml."
            }
            Es => {
                "El modo daemon está deshabilitado. Habilítalo en Ajustes o establece enabled = true bajo [daemon] en config.toml."
            }
            Fr => {
                "Le mode daemon est désactivé. Activez-le dans les Paramètres ou définissez enabled = true sous [daemon] dans config.toml."
            }
            De => {
                "Der Daemon-Modus ist deaktiviert. Aktivieren Sie ihn in den Einstellungen oder setzen Sie enabled = true unter [daemon] in config.toml."
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
        Msg::ConfigUnreadableRefused => match lang {
            En => "config.toml could not be read; fix or remove it and try again",
            It => "Impossibile leggere config.toml; correggilo o rimuovilo e riprova",
            Es => "No se pudo leer config.toml; corríjalo o elimínelo y vuelva a intentarlo",
            Fr => "Impossible de lire config.toml ; corrigez-le ou supprimez-le, puis réessayez",
            De => {
                "config.toml konnte nicht gelesen werden; korrigieren oder entfernen Sie die Datei und versuchen Sie es erneut"
            }
            Zh => "无法读取 config.toml；请修复或删除该文件后重试",
        },
        Msg::ConfigLoadFailed => match lang {
            En => {
                "config.toml could not be read; using defaults. Saving settings keeps the old file as a config.toml.unreadable-… copy"
            }
            It => {
                "Impossibile leggere config.toml; uso i valori predefiniti. Salvando le impostazioni il vecchio file resta come copia config.toml.unreadable-…"
            }
            Es => {
                "No se pudo leer config.toml; se usan los valores predeterminados. Al guardar los ajustes, el archivo anterior se conserva como copia config.toml.unreadable-…"
            }
            Fr => {
                "Impossible de lire config.toml ; valeurs par défaut utilisées. L'enregistrement des paramètres conserve l'ancien fichier en copie config.toml.unreadable-…"
            }
            De => {
                "config.toml konnte nicht gelesen werden; es gelten die Standardwerte. Beim Speichern bleibt die alte Datei als Kopie config.toml.unreadable-… erhalten"
            }
            Zh => {
                "无法读取 config.toml；正在使用默认值。保存设置时旧文件会保留为 config.toml.unreadable-… 副本"
            }
        },
        Msg::EncoderUnavailable => match lang {
            En => {
                "Warning: the selected encoder is missing from this FFmpeg build; every encode will fail."
            }
            It => {
                "Attenzione: l'encoder selezionato non è presente in questa build di FFmpeg; ogni conversione fallirà."
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
        Msg::DaemonTokenGeneratedSeeStatus => match lang {
            En => {
                "No strong access token was set, so one has been generated and saved to the config. Run av1converter --status to see the URL that authorises your browser."
            }
            It => {
                "Non era impostato un token di accesso sicuro: ne è stato generato uno e salvato nella configurazione. Esegui av1converter --status per vedere l'URL che autorizza il browser."
            }
            Es => {
                "No había un token de acceso seguro, así que se ha generado uno y guardado en la configuración. Ejecuta av1converter --status para ver la URL que autoriza tu navegador."
            }
            Fr => {
                "Aucun jeton d'accès robuste n'était défini : un jeton a été généré et enregistré dans la configuration. Lancez av1converter --status pour afficher l'URL qui autorise votre navigateur."
            }
            De => {
                "Es war kein starkes Zugriffstoken gesetzt, daher wurde eines erzeugt und in der Konfiguration gespeichert. Führen Sie av1converter --status aus, um die URL zu sehen, die Ihren Browser autorisiert."
            }
            Zh => {
                "未设置安全的访问令牌，已生成一个并保存到配置中。运行 av1converter --status 查看用于授权浏览器的网址。"
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
        Msg::DaemonPublicHttpRefused => match lang {
            En => {
                "Refusing a non-loopback bind over plain HTTP. Set daemon.allow_insecure_lan = true to opt in, or bind to 127.0.0.1 and use an HTTPS reverse proxy."
            }
            It => {
                "Rifiuto di un bind non-loopback su HTTP in chiaro. Imposta daemon.allow_insecure_lan = true per accettare, oppure usa 127.0.0.1 con un reverse proxy HTTPS."
            }
            Es => {
                "Se rechaza un bind fuera de loopback sobre HTTP sin cifrar. Pon daemon.allow_insecure_lan = true para aceptarlo, o usa 127.0.0.1 con un proxy HTTPS."
            }
            Fr => {
                "Refus d'une écoute hors loopback en HTTP clair. Définissez daemon.allow_insecure_lan = true pour accepter, ou écoutez sur 127.0.0.1 derrière un reverse proxy HTTPS."
            }
            De => {
                "Nicht-Loopback-Bind über Klartext-HTTP wird abgelehnt. Setzen Sie daemon.allow_insecure_lan = true, oder binden Sie an 127.0.0.1 hinter einem HTTPS-Reverse-Proxy."
            }
            Zh => {
                "拒绝在非回环地址上使用明文 HTTP。请设置 daemon.allow_insecure_lan = true，或绑定到 127.0.0.1 并使用 HTTPS 反向代理。"
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
        Msg::DaemonServiceStillStarting => match lang {
            En => {
                "The daemon is still starting; run av1converter --status in a moment to check it."
            }
            It => {
                "Il daemon si sta ancora avviando; esegui av1converter --status tra poco per controllarlo."
            }
            Es => {
                "El daemon todavía se está iniciando; ejecuta av1converter --status en un momento para comprobarlo."
            }
            Fr => {
                "Le daemon démarre encore ; lancez av1converter --status dans un instant pour le vérifier."
            }
            De => "Der Daemon startet noch; prüfen Sie ihn gleich mit av1converter --status.",
            Zh => "守护进程仍在启动；稍后运行 av1converter --status 查看状态。",
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
            Es => "El arranque al iniciar sesión solo está disponible en Linux (systemd) y macOS",
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
        Msg::WebReasonRestartInterrupted => match lang {
            En => "interrupted by a daemon restart",
            It => "interrotto da un riavvio del daemon",
            Es => "interrumpido por un reinicio del daemon",
            Fr => "interrompu par un redémarrage du daemon",
            De => "durch einen Daemon-Neustart unterbrochen",
            Zh => "被守护进程重启中断",
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
        Msg::WebKindDiscImage => match lang {
            En => "disc image",
            It => "immagine disco",
            Es => "imagen de disco",
            Fr => "image disque",
            De => "Disc-Abbild",
            Zh => "光盘映像",
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
        Msg::WebDvSourceHintPlain => match lang {
            En => "Dolby Vision source.",
            It => "Sorgente Dolby Vision.",
            Es => "Fuente Dolby Vision.",
            Fr => "Source Dolby Vision.",
            De => "Dolby-Vision-Quelle.",
            Zh => "杜比视界源。",
        },
        Msg::WebDvHint => match lang {
            En | It | Es | Fr | De => "{source} {detail}.",
            Zh => "{source}{detail}。",
        },
        Msg::WebDvProfile => match lang {
            En => "profile {n}",
            It => "profilo {n}",
            Es => "perfil {n}",
            Fr => "profil {n}",
            De => "Profil {n}",
            Zh => "Profile {n}",
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
            En => "Nothing was added",
            It => "Nessun file aggiunto",
            Es => "No se añadió nada",
            Fr => "Rien n'a été ajouté",
            De => "Nichts hinzugefügt",
            Zh => "未添加任何文件",
        },
        Msg::WebSkippedFiles => match lang {
            En => "{n} skipped",
            It => "{n} saltati",
            Es => "{n} omitidos",
            Fr => "{n} ignoré(s)",
            De => "{n} übersprungen",
            Zh => "{n} 个已跳过",
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
            De => "Discs auslesen",
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
            Fr => "Sélectionner toutes les pistes en repli",
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
                "Le modifiche a encoder, qualità e output valgono per i lavori in attesa; le tracce predefinite solo per i nuovi file."
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
            Fr => "Enregistrer les paramètres",
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
            Zh => "确认轨道",
        },
        Msg::WebSessionTotals => match lang {
            En => "Session totals",
            It => "Totali sessione",
            Es => "Totales de la sesión",
            Fr => "Totaux de la session",
            De => "Sitzungssummen",
            Zh => "会话总计",
        },
        Msg::WebSessionSummary => match lang {
            En => {
                "{headline} — {totals}: {converted} converted, {skipped} skipped, {errors} failed"
            }
            It => {
                "{headline} — {totals}: {converted} convertiti, {skipped} saltati, {errors} falliti"
            }
            Es => {
                "{headline} — {totals}: {converted} convertidos, {skipped} omitidos, {errors} fallidos"
            }
            Fr => {
                "{headline} — {totals} : {converted} convertis, {skipped} ignorés, {errors} en échec"
            }
            De => {
                "{headline} — {totals}: {converted} konvertiert, {skipped} übersprungen, {errors} fehlgeschlagen"
            }
            Zh => "{headline} — {totals}：已转换 {converted}，已跳过 {skipped}，失败 {errors}",
        },
        Msg::WebSessionSummaryCancelled => match lang {
            En => {
                "{headline} — {totals}: {converted} converted, {skipped} skipped, {cancelled} stopped, {errors} failed"
            }
            It => {
                "{headline} — {totals}: {converted} convertiti, {skipped} saltati, {cancelled} interrotti, {errors} falliti"
            }
            Es => {
                "{headline} — {totals}: {converted} convertidos, {skipped} omitidos, {cancelled} detenidos, {errors} fallidos"
            }
            Fr => {
                "{headline} — {totals} : {converted} convertis, {skipped} ignorés, {cancelled} arrêtés, {errors} en échec"
            }
            De => {
                "{headline} — {totals}: {converted} konvertiert, {skipped} übersprungen, {cancelled} gestoppt, {errors} fehlgeschlagen"
            }
            Zh => {
                "{headline} — {totals}：已转换 {converted}，已跳过 {skipped}，已停止 {cancelled}，失败 {errors}"
            }
        },
        Msg::WebApplyRemainingHint => match lang {
            En => "Matches tracks by order — best for files with the same track layout.",
            It => "Abbina le tracce in ordine — ideale per file con lo stesso schema di tracce.",
            Es => {
                "Empareja las pistas por orden — ideal para archivos con la misma disposición de pistas."
            }
            Fr => "Associe les pistes dans l'ordre — idéal pour des fichiers de même structure.",
            De => "Ordnet Spuren nach Reihenfolge zu — am besten bei gleichem Spurenaufbau.",
            Zh => "按顺序匹配轨道——最适合轨道结构相同的文件。",
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
                "Permanente: superato il controllo qualità video, la sorgente viene eliminata. Audio, sottotitoli e metadati non vengono verificati."
            }
            Es => {
                "Permanente: tras superar la calidad de video, se elimina el origen. Audio, subtítulos y metadatos no se verifican."
            }
            Fr => {
                "Permanent : après validation de la qualité vidéo, la source est supprimée. Audio, sous-titres et métadonnées ne sont pas vérifiés."
            }
            De => {
                "Dauerhaft: Nach bestandener Videoqualitätsprüfung wird die Quelle gelöscht. Audio, Untertitel und Metadaten werden nicht geprüft."
            }
            Zh => "永久操作：视频质量检查通过后将删除源文件。音频、字幕和元数据不会进行质量检查。",
        },
        Msg::WebRemoveRipPrompt => match lang {
            En => "Remove this job? Its ripped disc file will be deleted.",
            It => "Rimuovere questo lavoro? Il file estratto dal disco verrà eliminato.",
            Es => "¿Quitar este trabajo? Se eliminará el archivo extraído del disco.",
            Fr => "Retirer cette tâche ? Le fichier extrait du disque sera supprimé.",
            De => "Diesen Auftrag entfernen? Die von der Disc ausgelesene Datei wird gelöscht.",
            Zh => "移除此任务？从光盘翻录的文件将被删除。",
        },
        Msg::WebClearFinishedRipPrompt => match lang {
            En => "Clear finished jobs? Ripped disc files among them will be deleted.",
            It => {
                "Rimuovere i lavori completati? I file estratti dal disco tra questi verranno eliminati."
            }
            Es => {
                "¿Quitar los trabajos terminados? Se eliminarán los archivos extraídos del disco entre ellos."
            }
            Fr => {
                "Retirer les tâches terminées ? Les fichiers extraits du disque parmi elles seront supprimés."
            }
            De => {
                "Abgeschlossene Aufträge entfernen? Darunter befindliche, von der Disc ausgelesene Dateien werden gelöscht."
            }
            Zh => "清除已完成的任务？其中从光盘翻录的文件将被删除。",
        },
        Msg::WebDurationHoursMinutes => match lang {
            En => "{h}h {m}m",
            It | Es | Fr => "{h} h {m} min",
            De => "{h} Std. {m} Min.",
            Zh => "{h}小时{m}分钟",
        },
        Msg::WebDurationMinutesSeconds => match lang {
            En => "{m}m {s}s",
            It | Es | Fr => "{m} min {s} s",
            De => "{m} Min. {s} Sek.",
            Zh => "{m}分钟{s}秒",
        },
        Msg::WebDurationSeconds => match lang {
            En => "{s}s",
            It | Es | Fr => "{s} s",
            De => "{s} Sek.",
            Zh => "{s}秒",
        },
        Msg::TerminalTooSmall => match lang {
            En => "Terminal too small. Resize to at least {w} × {h}. Press q to quit.",
            It => {
                "Terminale troppo piccolo. Ridimensionalo ad almeno {w} × {h}. Premi q per uscire."
            }
            Es => "Terminal demasiado pequeño. Ajústalo al menos a {w} × {h}. Pulsa q para salir.",
            Fr => {
                "Terminal trop petit. Redimensionnez-le à au moins {w} × {h}. Appuyez sur q pour quitter."
            }
            De => "Terminal zu klein. Auf mindestens {w} × {h} vergrößern. q zum Beenden drücken.",
            Zh => "终端太小。请调整到至少 {w} × {h}。按 q 退出。",
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
    ("confirm", Msg::Confirm),
    ("disc_busy", Msg::DiscOperationRunning),
    ("disc_chapters", Msg::DiscChapterCount),
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
    ("selected", Msg::SelectedCount),
    ("status_ripping", Msg::StatusRipping),
    ("cancel_encoding", Msg::CancelEncodingTitle),
    ("cancel_encoding_prompt", Msg::CancelEncodingPrompt),
    ("cancel_disc", Msg::CancelDiscTitle),
    ("cancel_disc_prompt", Msg::CancelDiscPrompt),
    ("cancel_analysis", Msg::CancelAnalysisTitle),
    ("cancel_analysis_prompt", Msg::CancelAnalysisPrompt),
    ("abandon_tracks_prompt", Msg::AbandonTrackConfigPrompt),
    ("back", Msg::Back),
    ("cancelling", Msg::WebCancelling),
    ("cfg_audio_default", Msg::WebCfgAudioDefault),
    ("cfg_audio_languages", Msg::CfgAudioLanguages),
    ("cfg_audio_mode_copy", Msg::WebCfgAudioModeCopy),
    ("cfg_audio_mode_opus", Msg::WebCfgAudioModeOpus),
    ("cfg_delete_source", Msg::CfgDeleteSource),
    ("cfg_daemon_auth_token", Msg::CfgDaemonAuthToken),
    (
        "cfg_daemon_allow_insecure_lan",
        Msg::CfgDaemonAllowInsecureLan,
    ),
    ("cfg_daemon_behind_proxy", Msg::CfgDaemonBehindProxy),
    ("cfg_daemon_autostart", Msg::CfgDaemonAutostart),
    ("cfg_daemon_bind_address", Msg::CfgDaemonBindAddress),
    ("cfg_daemon_browse_root", Msg::CfgDaemonBrowseRoot),
    ("cfg_daemon_enabled", Msg::CfgDaemonEnabled),
    ("cfg_daemon_port", Msg::CfgDaemonPort),
    ("cfg_encoder", Msg::EncoderLabel),
    ("encoder_unavailable", Msg::EncoderUnavailable),
    ("vmaf_disabled", Msg::VmafDisabled),
    ("vmaf_enabled_open", Msg::VmafEnabledOpen),
    ("deps_missing", Msg::DepsNotAvailable),
    ("opus_unavailable", Msg::OpusUnavailable),
    ("cfg_film_grain", Msg::CfgFilmGrain),
    ("cfg_language", Msg::CfgLanguage),
    ("cfg_makemkvcon_path", Msg::CfgMakemkvconPath),
    ("cfg_nvenc_preset", Msg::CfgNvencPreset),
    ("cfg_qsv_quality", Msg::CfgQsvQuality),
    ("cfg_amf_quality", Msg::CfgAmfQuality),
    ("encoder_svt_av1", Msg::EncoderSvtAv1),
    ("vmaf_min_score", Msg::VmafMinScore),
    ("nav_primary", Msg::WebPrimaryNav),
    ("request_failed", Msg::WebRequestFailed),
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
    ("dv_hdr10_desc", Msg::DvOptionHdr10Desc),
    ("dv_keep", Msg::DvOptionKeep),
    ("dv_keep_desc", Msg::DvOptionKeepDesc),
    ("dv_p5_warning", Msg::DvP5Warning),
    ("dv_recommended", Msg::DvRecommended),
    ("dv_profile", Msg::WebDvProfile),
    ("dv_remux_hint", Msg::WebDvRemuxHint),
    ("dv_requires_svt", Msg::DvRequiresSvt),
    ("dv_source_hint", Msg::WebDvSourceHint),
    ("dv_source_hint_plain", Msg::WebDvSourceHintPlain),
    ("dv_hint", Msg::WebDvHint),
    ("eta", Msg::Eta),
    ("elapsed", Msg::Elapsed),
    ("threshold_label", Msg::ThresholdLabel),
    ("tag_source_kept", Msg::SourceKeptTag),
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
    ("kind_disc_image", Msg::WebKindDiscImage),
    ("kind_not_selectable", Msg::WebKindNotSelectable),
    ("kind_symlink", Msg::WebKindSymlink),
    ("kind_video", Msg::WebKindVideoFile),
    ("move_up", Msg::MoveUp),
    ("no_audio_tracks", Msg::WebNoAudioTracks),
    ("no_subtitle_tracks", Msg::WebNoSubtitleTracks),
    ("nothing_added", Msg::WebNothingAdded),
    ("skipped_files", Msg::WebSkippedFiles),
    ("offline", Msg::WebOffline),
    ("options", Msg::WebOptions),
    ("overall_progress", Msg::WebOverallProgress),
    ("parent_directory", Msg::WebParentDirectory),
    ("qp_custom", Msg::QpCustom),
    ("qp_high", Msg::QpHigh),
    ("qp_low", Msg::QpLow),
    ("qp_medium", Msg::QpMedium),
    ("queue_empty", Msg::WebQueueEmpty),
    ("queue_unreadable", Msg::QueueUnreadable),
    ("reason_cancelled", Msg::Cancelled),
    (
        "reason_restart_interrupted",
        Msg::WebReasonRestartInterrupted,
    ),
    ("remove_from_queue", Msg::WebRemoveFromQueue),
    ("remove_rip_prompt", Msg::WebRemoveRipPrompt),
    ("clear_finished_rip_prompt", Msg::WebClearFinishedRipPrompt),
    ("duration_hm", Msg::WebDurationHoursMinutes),
    ("duration_ms", Msg::WebDurationMinutesSeconds),
    ("duration_s", Msg::WebDurationSeconds),
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
    ("summary_cancelled", Msg::Cancelled),
    ("summary_dismiss", Msg::WebDismissSummary),
    ("summary_errors", Msg::Errors),
    ("summary_time", Msg::TotalTime),
    ("tab_queue", Msg::WebTabQueue),
    ("tab_settings", Msg::Settings),
    ("subtitle_not_included", Msg::SubtitleNotIncluded),
    ("tag_remux", Msg::WebTagRemux),
    ("tag_source_deleted", Msg::SourceDeletedTag),
    ("all_opus", Msg::AllOpus),
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
    ("session_summary", Msg::WebSessionSummary),
    ("session_summary_cancelled", Msg::WebSessionSummaryCancelled),
    ("apply_remaining_hint", Msg::WebApplyRemainingHint),
    ("autostart_hint", Msg::CfgDaemonAutostartHint),
    ("autostart_unsupported", Msg::DaemonServiceUnsupported),
    ("saving", Msg::WebSaving),
    ("browse", Msg::WebBrowse),
    ("loading", Msg::WebLoading),
    ("summary_failed", Msg::WebFinishedWithErrors),
    ("summary_stopped", Msg::WebConversionStopped),
    ("verifying_vmaf", Msg::StatusVerifying),
];

/// Get quality description for VMAF score
pub fn quality_description(lang: Language, score: f64) -> &'static str {
    let msg = if score >= 95.0 {
        Msg::QualExcellent
    } else if score >= 90.0 {
        Msg::QualVeryGood
    } else if score >= 85.0 {
        Msg::QualGood
    } else if score >= 80.0 {
        Msg::QualFair
    } else if score >= 70.0 {
        Msg::QualPoor
    } else {
        Msg::QualBad
    };
    t(lang, msg)
}

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

    /// Every `{…}` group in a string, in order of appearance.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            found.push(&rest[open..=open + close]);
            rest = &rest[open + close + 1..];
        }
        found
    }

    /// Every translation keeps the placeholders the browser substitutes.
    #[test]
    fn placeholders_survive_every_translation() {
        for (key, msg) in WEB_KEYS {
            let english = placeholders(t(Language::English, *msg));
            if english.is_empty() {
                continue;
            }
            for &lang in &Language::ALL {
                let mut theirs = placeholders(t(lang, *msg));
                let mut ours = english.clone();
                theirs.sort_unstable();
                ours.sort_unstable();
                assert_eq!(
                    ours, theirs,
                    "web key {key} has placeholders {theirs:?} in {lang:?}, expected {ours:?}"
                );
            }
        }
    }

    /// The placeholder scanner finds every group and ignores an unclosed one.
    #[test]
    fn placeholders_are_collected_in_order() {
        assert_eq!(placeholders("{a} x {bb} y"), vec!["{a}", "{bb}"]);
        assert_eq!(placeholders("no groups"), Vec::<&str>::new());
        assert_eq!(placeholders("{n} and {open"), vec!["{n}"]);
    }
}
