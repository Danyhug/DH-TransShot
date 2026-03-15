use crate::config::{AppState, Settings};
use log::{info, error};
use tauri::State;

/// Get the current settings.
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    info!("[Settings] get_settings 请求");
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    info!("[Settings] 返回配置, translation.model={}, ocr.model={}", settings.translation.model, settings.ocr.model);
    Ok(settings.clone())
}

/// Save settings.
#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    info!(
        "[Settings] save_settings, translation.model={}, ocr.model={}, tts.model={}",
        settings.translation.model,
        settings.ocr.model,
        settings.tts.model
    );
    // Update in-memory state
    {
        let mut current = state.settings.lock().map_err(|e| e.to_string())?;
        *current = settings.clone();
    }
    {
        let mut tts_cache = state.tts_cache.lock().map_err(|e| e.to_string())?;
        tts_cache.clear();
    }
    info!("[Settings] TTS 缓存已清空");
    // Persist to store
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    store.set("settings", value);
    store.save().map_err(|e| {
        error!("[Settings] 持久化保存失败: {}", e);
        e.to_string()
    })?;
    info!("[Settings] 配置保存成功");
    Ok(())
}
