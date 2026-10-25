use std::{cell::RefCell, rc::Rc};

use super::{LanguageConfig, Rope, SyntaxContext, SyntaxContextProvider};
use gpui::{App, Global, SharedString};

pub trait LanguageProvider {
    fn language_name(&self, name: &str) -> SharedString {
        name.to_lowercase().into()
    }

    fn config(&self, _name: &str) -> Rc<LanguageConfig> {
        Rc::new(LanguageConfig::default())
    }

    fn syntax_context_provider(&self, _name: &str) -> Option<Rc<dyn SyntaxContextProvider>> {
        None
    }
}

#[derive(Default)]
struct DefaultLanguages(Rc<LanguageConfig>);
impl LanguageProvider for DefaultLanguages {
    fn config(&self, _: &str) -> Rc<LanguageConfig> {
        self.0.clone()
    }
}

struct LanguageSettings {
    provider: Rc<dyn LanguageProvider>,
    // Registration order preserves precedence when a replacement service resolves aliases.
    configs: Vec<(SharedString, Rc<LanguageConfig>)>,
}

impl Default for LanguageSettings {
    fn default() -> Self {
        Self {
            provider: Rc::new(DefaultLanguages::default()),
            configs: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Languages(Rc<RefCell<LanguageSettings>>);
impl Global for Languages {}

impl Languages {
    fn global(cx: &mut App) -> Self {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.global::<Self>().clone()
    }
}

pub fn set_language_provider(provider: Rc<dyn LanguageProvider>, cx: &mut App) {
    Languages::global(cx).0.borrow_mut().provider = provider;
}

pub fn set_language_config(language: impl AsRef<str>, config: LanguageConfig, cx: &mut App) {
    let languages = Languages::global(cx);
    let name: SharedString = language.as_ref().to_string().into();
    let mut settings = languages.0.borrow_mut();
    settings
        .configs
        .retain(|(registered, _)| *registered != name);
    settings.configs.push((name, Rc::new(config)));
}

struct LanguageSyntax {
    source: Rc<dyn LanguageProvider>,
    provider: Option<Rc<dyn SyntaxContextProvider>>,
}

#[derive(Clone, Default)]
pub(crate) struct EditorLanguage {
    name: SharedString,
    languages: Languages,
    syntax: Rc<RefCell<Option<LanguageSyntax>>>,
}

impl EditorLanguage {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            name: SharedString::default(),
            languages: Languages::global(cx),
            syntax: Rc::default(),
        }
    }

    pub(crate) fn name(&self) -> SharedString {
        self.name.clone()
    }

    pub(crate) fn set_name(&mut self, name: SharedString) {
        self.name = name;
        self.syntax = Rc::new(RefCell::new(None));
    }

    pub(crate) fn config(&self) -> Rc<LanguageConfig> {
        let source = self.languages.0.borrow().provider.clone();
        let name = source.language_name(&self.name);
        let configured = self
            .languages
            .0
            .borrow()
            .configs
            .iter()
            .rev()
            .find(|(registered, _)| source.language_name(registered) == name)
            .map(|(_, config)| config.clone());
        configured.unwrap_or_else(|| source.config(&name))
    }

    pub(crate) fn context_at(&self, text: &Rope, offset: usize) -> SyntaxContext {
        let source = self.languages.0.borrow().provider.clone();
        let mut syntax = self.syntax.borrow_mut();
        if syntax
            .as_ref()
            .is_none_or(|cached| !Rc::ptr_eq(&cached.source, &source))
        {
            let name = source.language_name(&self.name);
            *syntax = Some(LanguageSyntax {
                provider: source.syntax_context_provider(&name),
                source,
            });
        }
        let provider = syntax.as_ref().and_then(|syntax| syntax.provider.clone());
        drop(syntax);
        provider.map_or(SyntaxContext::Code, |provider| {
            provider.context_at(text, offset)
        })
    }
}
