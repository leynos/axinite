//! Extension and higher-level feature-tool registration helpers.

use std::path::PathBuf;
use std::sync::Arc;

use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::builtin::{
    ExtensionInfoTool, SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool,
    ToolActivateTool, ToolAuthTool, ToolInstallTool, ToolListTool, ToolRemoveTool, ToolSearchTool,
    ToolUpgradeTool,
};
use crate::tools::tool::Tool;

use super::ToolRegistry;

/// Arguments for registering image-generation and image-editing tools.
#[derive(Clone, Debug)]
pub struct ImageToolsArgs {
    /// Base URL for the backing image API.
    pub api_base_url: String,
    /// API key used by the image tools.
    pub api_key: String,
    /// Model identifier for image generation and editing requests.
    pub gen_model: String,
    /// Optional workspace-relative base directory for file-backed image edits.
    pub base_dir: Option<PathBuf>,
}

/// Arguments for registering vision/image-analysis tools.
#[derive(Clone, Debug)]
pub struct VisionToolsArgs {
    /// Base URL for the backing vision API.
    pub api_base_url: String,
    /// API key used by the vision tool.
    pub api_key: String,
    /// Model identifier for vision analysis requests.
    pub vision_model: String,
    /// Optional workspace-relative base directory for reading local images.
    pub base_dir: Option<PathBuf>,
}

impl ToolRegistry {
    /// Register extension management tools (search, install, auth, activate, list, remove).
    ///
    /// These allow the LLM to manage MCP servers and WASM tools through conversation.
    pub fn register_extension_tools(&self, manager: Arc<ExtensionManager>) {
        self.register_sync(Arc::new(ToolSearchTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolInstallTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolAuthTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolActivateTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolListTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolRemoveTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ToolUpgradeTool::new(Arc::clone(&manager))));
        self.register_sync(Arc::new(ExtensionInfoTool::new(manager)));
        tracing::debug!("Registered 8 extension management tools");
    }

    /// Register skill management tools (list, search, install, remove).
    ///
    /// These allow the LLM to manage prompt-level skills through conversation.
    pub fn register_skill_tools(
        &self,
        registry: Arc<std::sync::RwLock<SkillRegistry>>,
        catalog: Arc<SkillCatalog>,
    ) {
        self.register_sync(Arc::new(SkillListTool::new(Arc::clone(&registry))));
        self.register_sync(Arc::new(SkillSearchTool::new(
            Arc::clone(&registry),
            Arc::clone(&catalog),
        )));
        self.register_sync(Arc::new(SkillInstallTool::new(
            Arc::clone(&registry),
            Arc::clone(&catalog),
        )));
        self.register_sync(Arc::new(SkillRemoveTool::new(registry)));
        tracing::debug!("Registered 4 skill management tools");
    }

    /// Register routine management tools.
    ///
    /// These allow the LLM to create, list, update, delete, and view history
    /// of routines (scheduled and event-driven tasks).
    pub fn register_routine_tools(
        &self,
        store: Arc<dyn Database>,
        engine: Arc<crate::agent::routine_engine::RoutineEngine>,
    ) {
        use crate::tools::builtin::{
            EventEmitTool, RoutineCreateTool, RoutineDeleteTool, RoutineFireTool,
            RoutineHistoryTool, RoutineListTool, RoutineUpdateTool,
        };
        self.register_sync(Arc::new(RoutineCreateTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineListTool::new(Arc::clone(&store))));
        self.register_sync(Arc::new(RoutineUpdateTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineDeleteTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineFireTool::new(
            Arc::clone(&store),
            Arc::clone(&engine),
        )));
        self.register_sync(Arc::new(RoutineHistoryTool::new(store)));
        self.register_sync(Arc::new(EventEmitTool::new(engine)));
        tracing::debug!("Registered 7 routine management tools");
    }

    /// Register message tool for sending messages to channels.
    pub async fn register_message_tools(
        &self,
        channel_manager: Arc<crate::channels::ChannelManager>,
    ) {
        use crate::tools::builtin::MessageTool;
        let tool = Arc::new(MessageTool::new(channel_manager));
        let tool_name = tool.name().to_string();
        *self.message_tool.write().await = Some(Arc::clone(&tool));
        self.tools
            .write()
            .await
            .insert(tool_name.clone(), tool as Arc<dyn Tool>);
        self.builtin_names.write().await.insert(tool_name);
        tracing::debug!("Registered message tool");
    }

    /// Register image generation and editing tools.
    ///
    /// These tools allow the LLM to generate and edit images using cloud APIs.
    pub fn register_image_tools(&self, args: ImageToolsArgs) {
        use crate::tools::builtin::{ImageEditTool, ImageGenerateTool};
        let ImageToolsArgs {
            api_base_url,
            api_key,
            gen_model,
            base_dir,
        } = args;
        self.register_sync(Arc::new(ImageGenerateTool::new(
            api_base_url.clone(),
            api_key.clone(),
            gen_model.clone(),
        )));
        self.register_sync(Arc::new(ImageEditTool::new(
            api_base_url,
            api_key,
            gen_model,
            base_dir,
        )));
        tracing::debug!("Registered 2 image tools (generate, edit)");
    }

    /// Register vision/image analysis tools.
    ///
    /// These tools allow the LLM to analyze images using a vision-capable model.
    pub fn register_vision_tools(&self, args: VisionToolsArgs) {
        use crate::tools::builtin::ImageAnalyzeTool;
        let VisionToolsArgs {
            api_base_url,
            api_key,
            vision_model,
            base_dir,
        } = args;
        self.register_sync(Arc::new(ImageAnalyzeTool::new(
            api_base_url,
            api_key,
            vision_model,
            base_dir,
        )));
        tracing::debug!("Registered 1 vision tool (analyze)");
    }
}
