use axum::{
    extract::{Json, Path, Form},
    http::StatusCode,
    response::{IntoResponse, Response, Html},
    routing::{post, get, delete, put},
    Router,
};
use uuid::Uuid;
use dragonfly_common::*;
use dragonfly_common::models::{HostnameUpdateRequest, HostnameUpdateResponse, OsInstalledUpdateRequest, OsInstalledUpdateResponse, BmcCredentialsUpdateRequest, BmcCredentials, BmcType};
use tracing::{error, info, warn};
use serde_json::json;
use serde::Deserialize;

use crate::db;

pub fn api_router() -> Router {
    Router::new()
        .route("/api/machines", post(register_machine))
        .route("/api/machines", get(get_all_machines))
        .route("/api/machines/:id", get(get_machine))
        .route("/api/machines/:id", delete(delete_machine))
        .route("/api/machines/:id", put(update_machine))
        .route("/api/machines/:id/os", post(assign_os))
        .route("/api/machines/:id/status", post(update_status))
        .route("/api/machines/:id/hostname", post(update_hostname))
        .route("/api/machines/:id/hostname", get(get_hostname_form))
        .route("/api/machines/:id/os_installed", post(update_os_installed))
        .route("/api/machines/:id/bmc", post(update_bmc))
        .route("/:mac", get(ipxe_script))
}

async fn register_machine(
    Json(payload): Json<RegisterRequest>,
) -> Response {
    info!("Registering machine with MAC: {}", payload.mac_address);
    
    match db::register_machine(&payload).await {
        Ok(machine_id) => {
            // Get the new machine to register with Tinkerbell
            if let Ok(Some(machine)) = db::get_machine_by_id(&machine_id).await {
                // Register with Tinkerbell (don't fail if this fails)
                if let Err(e) = crate::tinkerbell::register_machine(&machine).await {
                    warn!("Failed to register machine with Tinkerbell (continuing anyway): {}", e);
                }
            }
            
            let response = RegisterResponse {
                machine_id,
                next_step: "awaiting_os_assignment".to_string(),
            };
            (StatusCode::CREATED, Json(response)).into_response()
        },
        Err(e) => {
            error!("Failed to register machine: {}", e);
            let error_response = ErrorResponse {
                error: "Registration Failed".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn get_all_machines() -> Response {
    match db::get_all_machines().await {
        Ok(machines) => {
            (StatusCode::OK, Json(machines)).into_response()
        },
        Err(e) => {
            error!("Failed to retrieve machines: {}", e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn get_machine(
    Path(id): Path<Uuid>,
) -> Response {
    match db::get_machine_by_id(&id).await {
        Ok(Some(machine)) => {
            (StatusCode::OK, Json(machine)).into_response()
        },
        Ok(None) => {
            let error_response = ErrorResponse {
                error: "Not Found".to_string(),
                message: format!("Machine with ID {} not found", id),
            };
            (StatusCode::NOT_FOUND, Json(error_response)).into_response()
        },
        Err(e) => {
            error!("Failed to retrieve machine {}: {}", id, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn assign_os(
    Path(id): Path<Uuid>,
    Json(payload): Json<OsAssignmentRequest>,
) -> Response {
    info!("Assigning OS {} to machine {}", payload.os_choice, id);
    
    match db::assign_os(&id, &payload.os_choice).await {
        Ok(true) => {
            let response = OsAssignmentResponse {
                success: true,
                message: format!("OS {} assigned to machine {}", payload.os_choice, id),
            };
            (StatusCode::OK, Json(response)).into_response()
        },
        Ok(false) => {
            let error_response = ErrorResponse {
                error: "Not Found".to_string(),
                message: format!("Machine with ID {} not found", id),
            };
            (StatusCode::NOT_FOUND, Json(error_response)).into_response()
        },
        Err(e) => {
            error!("Failed to assign OS to machine {}: {}", id, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn update_status(
    Path(id): Path<Uuid>,
    Json(payload): Json<StatusUpdateRequest>,
) -> Response {
    info!("Updating status for machine {} to {:?}", id, payload.status);
    
    match db::update_status(&id, payload.status).await {
        Ok(true) => {
            // Get the updated machine to update Tinkerbell
            if let Ok(Some(machine)) = db::get_machine_by_id(&id).await {
                // Update the machine in Tinkerbell (don't fail if this fails)
                if let Err(e) = crate::tinkerbell::register_machine(&machine).await {
                    warn!("Failed to update machine in Tinkerbell (continuing anyway): {}", e);
                }
            }
            
            let response = StatusUpdateResponse {
                success: true,
                message: format!("Status updated for machine {}", id),
            };
            (StatusCode::OK, Json(response)).into_response()
        },
        Ok(false) => {
            let error_response = ErrorResponse {
                error: "Not Found".to_string(),
                message: format!("Machine with ID {} not found", id),
            };
            (StatusCode::NOT_FOUND, Json(error_response)).into_response()
        },
        Err(e) => {
            error!("Failed to update status for machine {}: {}", id, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn update_hostname(
    Path(id): Path<Uuid>,
    Json(payload): Json<HostnameUpdateRequest>,
) -> Response {
    info!("Updating hostname for machine {} to {}", id, payload.hostname);
    
    match db::update_hostname(&id, &payload.hostname).await {
        Ok(true) => {
            // Get the updated machine to update Tinkerbell
            if let Ok(Some(machine)) = db::get_machine_by_id(&id).await {
                // Update the machine in Tinkerbell (don't fail if this fails)
                if let Err(e) = crate::tinkerbell::register_machine(&machine).await {
                    warn!("Failed to update machine in Tinkerbell (continuing anyway): {}", e);
                }
            }
            
            let response = HostnameUpdateResponse {
                success: true,
                message: format!("Hostname updated for machine {}", id),
            };
            (StatusCode::OK, Json(response)).into_response()
        },
        Ok(false) => {
            let error_response = ErrorResponse {
                error: "Not Found".to_string(),
                message: format!("Machine with ID {} not found", id),
            };
            (StatusCode::NOT_FOUND, Json(error_response)).into_response()
        },
        Err(e) => {
            error!("Failed to update hostname for machine {}: {}", id, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn update_os_installed(
    Path(id): Path<Uuid>,
    Json(payload): Json<OsInstalledUpdateRequest>,
) -> Response {
    info!("Updating OS installed for machine {} to {}", id, payload.os_installed);
    
    match db::update_os_installed(&id, &payload.os_installed).await {
        Ok(true) => {
            let response = OsInstalledUpdateResponse {
                success: true,
                message: format!("OS installed updated for machine {}", id),
            };
            (StatusCode::OK, Json(response)).into_response()
        },
        Ok(false) => {
            let error_response = ErrorResponse {
                error: "Not Found".to_string(),
                message: format!("Machine with ID {} not found", id),
            };
            (StatusCode::NOT_FOUND, Json(error_response)).into_response()
        },
        Err(e) => {
            error!("Failed to update OS installed for machine {}: {}", id, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

async fn update_bmc(
    Path(id): Path<Uuid>,
    Form(payload): Form<BmcCredentialsUpdateRequest>,
) -> Response {
    info!("Updating BMC credentials for machine {}", id);
    
    // Create BMC credentials from the form data
    let bmc_type = match payload.bmc_type.as_str() {
        "IPMI" => BmcType::IPMI,
        "Redfish" => BmcType::Redfish,
        _ => BmcType::Other(payload.bmc_type),
    };
    
    let credentials = BmcCredentials {
        address: payload.bmc_address,
        username: payload.bmc_username,
        password: Some(payload.bmc_password),
        bmc_type,
    };
    
    match db::update_bmc_credentials(&id, &credentials).await {
        Ok(true) => {
            (StatusCode::OK, Html(format!(r#"
                <div class="p-4 mb-4 text-sm text-green-700 bg-green-100 rounded-lg" role="alert">
                    <span class="font-medium">Success!</span> BMC credentials updated.
                </div>
                <script>
                    setTimeout(function() {{
                        window.location.reload();
                    }}, 1500);
                </script>
            "#))).into_response()
        },
        Ok(false) => {
            let error_message = format!("Machine with ID {} not found", id);
            (StatusCode::NOT_FOUND, Html(format!(r#"
                <div class="p-4 mb-4 text-sm text-red-700 bg-red-100 rounded-lg" role="alert">
                    <span class="font-medium">Error!</span> {}.
                </div>
            "#, error_message))).into_response()
        },
        Err(e) => {
            error!("Failed to update BMC credentials for machine {}: {}", id, e);
            let error_message = format!("Database error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Html(format!(r#"
                <div class="p-4 mb-4 text-sm text-red-700 bg-red-100 rounded-lg" role="alert">
                    <span class="font-medium">Error!</span> {}.
                </div>
            "#, error_message))).into_response()
        }
    }
}

// Handler to get the hostname edit form
async fn get_hostname_form(
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match db::get_machine_by_id(&id).await {
        Ok(Some(machine)) => {
            let current_hostname = machine.hostname.unwrap_or_default();
            // Use raw string literals to avoid escaping issues
            let html = format!(
                r###"
                <div class="sm:flex sm:items-start">
                    <div class="mt-3 text-center sm:mt-0 sm:text-left w-full">
                        <h3 class="text-base font-semibold leading-6 text-gray-900">
                            Update Machine Hostname
                        </h3>
                        <div class="mt-2">
                            <form hx-post="/api/machines/{}/hostname" hx-target="#hostname-modal">
                                <label for="hostname" class="block text-sm font-medium text-gray-700">Hostname</label>
                                <input type="text" name="hostname" id="hostname" value="{}" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm" placeholder="Enter hostname">
                                <div class="mt-5 sm:mt-4 sm:flex sm:flex-row-reverse">
                                    <button type="submit" class="inline-flex w-full justify-center rounded-md bg-indigo-600 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-500 sm:ml-3 sm:w-auto">
                                        Update
                                    </button>
                                    <button type="button" class="mt-3 inline-flex w-full justify-center rounded-md bg-white px-3 py-2 text-sm font-semibold text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 hover:bg-gray-50 sm:mt-0 sm:w-auto" onclick="document.getElementById('hostname-modal').classList.add('hidden')">
                                        Cancel
                                    </button>
                                </div>
                            </form>
                        </div>
                    </div>
                </div>
                "###,
                id, current_hostname
            );
            
            (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/html")], html)
        },
        Ok(None) => {
            let error_html = format!(
                r###"<div class="p-4 text-red-500">Machine with ID {} not found</div>"###,
                id
            );
            (StatusCode::NOT_FOUND, [(axum::http::header::CONTENT_TYPE, "text/html")], error_html)
        },
        Err(e) => {
            let error_html = format!(
                r###"<div class="p-4 text-red-500">Error: {}</div>"###,
                e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, [(axum::http::header::CONTENT_TYPE, "text/html")], error_html)
        }
    }
}

// Handler for iPXE script generation
async fn ipxe_script(Path(mac): Path<String>) -> Response {
    if !mac.contains(':') || mac.split(':').count() != 6 {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }
    
    info!("Generating iPXE script for MAC: {}", mac);
    
    match db::get_machine_by_mac(&mac).await {
        Ok(Some(_)) => {
            let script = format!("#!ipxe\nchain http://10.7.1.30:8080/hookos.ipxe");
            (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/plain")], script).into_response()
        },
        Ok(None) => {
            let script = format!("#!ipxe\nchain http://10.7.1.30:8080/sparxplug.ipxe");
            (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "text/plain")], script).into_response()
        },
        Err(e) => {
            error!("Database error while looking up MAC {}: {}", mac, e);
            let error_response = ErrorResponse {
                error: "Database Error".to_string(),
                message: e.to_string(),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
        }
    }
}

// Update the delete_machine function to use kube-rs instead of kubectl
async fn delete_machine(
    Path(id): Path<Uuid>,
) -> Response {
    info!("Request to delete machine: {}", id);

    // Get the machine to find its MAC address
    match db::get_machine_by_id(&id).await {
        Ok(Some(machine)) => {
            // Delete from Tinkerbell
            let mac_address = machine.mac_address.replace(":", "-").to_lowercase();
            
            let tinkerbell_result = match crate::tinkerbell::delete_hardware(&mac_address).await {
                Ok(_) => {
                    info!("Successfully deleted machine from Tinkerbell: {}", mac_address);
                    true
                },
                Err(e) => {
                    warn!("Failed to delete machine from Tinkerbell: {}", e);
                    false
                }
            };

            // Delete from database
            match db::delete_machine(&id).await {
                Ok(true) => {
                    let message = if tinkerbell_result {
                        "Machine successfully deleted from Dragonfly and Tinkerbell."
                    } else {
                        "Machine deleted from Dragonfly but there was an issue removing it from Tinkerbell."
                    };
                    
                    (StatusCode::OK, Json(json!({ "success": true, "message": message }))).into_response()
                },
                Ok(false) => {
                    (StatusCode::NOT_FOUND, Json(json!({ "error": "Machine not found in database" }))).into_response()
                },
                Err(e) => {
                    error!("Failed to delete machine from database: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("Database error: {}", e) }))).into_response()
                }
            }
        },
        Ok(None) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Machine not found" }))).into_response()
        },
        Err(e) => {
            error!("Error fetching machine for deletion: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("Database error: {}", e) }))).into_response()
        }
    }
}

// Define a new request struct for the update machine operation
#[derive(Debug, Deserialize)]
struct UpdateMachineRequest {
    hostname: Option<String>,
    ip_address: Option<String>,
    mac_address: Option<String>,
    #[serde(rename = "nameservers[]")]
    nameservers: Option<Vec<String>>,
}

// Add this function to handle machine updates
async fn update_machine(
    Path(id): Path<Uuid>,
    Form(payload): Form<UpdateMachineRequest>,
) -> Response {
    info!("Updating machine {}", id);
    let mut updated = false;
    let mut messages = vec![];

    // Update hostname if provided
    if let Some(hostname) = &payload.hostname {
        if !hostname.is_empty() {
            match db::update_hostname(&id, hostname).await {
                Ok(true) => {
                    updated = true;
                    messages.push(format!("Hostname updated to '{}'", hostname));
                },
                Ok(false) => {
                    messages.push("Machine not found for hostname update".to_string());
                },
                Err(e) => {
                    error!("Failed to update hostname: {}", e);
                    messages.push(format!("Failed to update hostname: {}", e));
                }
            }
        }
    }

    // Update IP address if provided
    if let Some(ip_address) = &payload.ip_address {
        if !ip_address.is_empty() {
            match db::update_ip_address(&id, ip_address).await {
                Ok(true) => {
                    updated = true;
                    messages.push(format!("IP address updated to '{}'", ip_address));
                },
                Ok(false) => {
                    messages.push("Machine not found for IP address update".to_string());
                },
                Err(e) => {
                    error!("Failed to update IP address: {}", e);
                    messages.push(format!("Failed to update IP address: {}", e));
                }
            }
        }
    }
    
    // Update MAC address if provided
    if let Some(mac_address) = &payload.mac_address {
        if !mac_address.is_empty() {
            match db::update_mac_address(&id, mac_address).await {
                Ok(true) => {
                    updated = true;
                    messages.push(format!("MAC address updated to '{}'", mac_address));
                },
                Ok(false) => {
                    messages.push("Machine not found for MAC address update".to_string());
                },
                Err(e) => {
                    error!("Failed to update MAC address: {}", e);
                    messages.push(format!("Failed to update MAC address: {}", e));
                }
            }
        }
    }
    
    // Update DNS servers if provided
    if let Some(nameservers) = &payload.nameservers {
        // Filter out empty strings
        let filtered_nameservers: Vec<String> = nameservers.iter()
            .filter(|ns| !ns.is_empty())
            .cloned()
            .collect();
            
        if !filtered_nameservers.is_empty() {
            match db::update_nameservers(&id, &filtered_nameservers).await {
                Ok(true) => {
                    updated = true;
                    messages.push(format!("DNS servers updated"));
                },
                Ok(false) => {
                    messages.push("Machine not found for DNS servers update".to_string());
                },
                Err(e) => {
                    error!("Failed to update DNS servers: {}", e);
                    messages.push(format!("Failed to update DNS servers: {}", e));
                }
            }
        }
    }

    if updated {
        (StatusCode::OK, Json(json!({
            "success": true,
            "message": messages.join(", ")
        }))).into_response()
    } else {
        (StatusCode::BAD_REQUEST, Json(json!({
            "success": false,
            "message": if messages.is_empty() { "No updates provided".to_string() } else { messages.join(", ") }
        }))).into_response()
    }
}

// Error handling
pub async fn handle_error(err: anyhow::Error) -> Response {
    error!("Internal server error: {}", err);
    let error_response = ErrorResponse {
        error: "Internal Server Error".to_string(),
        message: err.to_string(),
    };

    (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
} 