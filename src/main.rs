use postgres::{ Client, NoTls };
use postgres::Error as PostgresError;
use std::net::{ TcpListener, TcpStream };
use std::io::{ Read, Write };
use std::env;

#[macro_use]
extern crate serde_derive;

// Define the model in a struct
#[derive(Serialize, Deserialize, Debug)]
struct User {
    pub id: i32,
    pub name: String,
    pub email: String,
}

// Define the NewUser struct
#[derive(Serialize, Deserialize, Debug)]
struct NewUser {
    pub name: String,
    pub email: String,
}

// Environment variables defined in the docker compose to connect ot the DB
const DB_URL: &'static str = env!("DATABASE_URL");

fn main() {
    // Set the database
    if let Err(_) = set_database() {
        return;
    }

    // Start the server
    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();

    // Handle the requests
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_client(stream),
            Err(e) => println!("Error: {}", e),
        }
    }
}

// Database setup: change this accordingly to the model
fn set_database() -> Result<(), PostgresError> {
    // Connect to the database
    let mut client = Client::connect(DB_URL, NoTls).unwrap();

    // Create the table
    client.batch_execute(
        "
        CREATE TABLE IF NOT EXISTS users (
            id              SERIAL PRIMARY KEY,
            name            VARCHAR NOT NULL,
            email           VARCHAR UNIQUE NOT NULL
        )"
    )?;

    Ok(())
}

// Handle the requests
fn handle_client(mut stream: TcpStream) {
    // Read the request
    let mut buffer = [0; 1024];
    let mut request = String::new();

    match stream.read(&mut buffer) {
        Ok(size) => {
            request.push_str(&String::from_utf8_lossy(&buffer[..size]));

            let (status_line, content) = match () {
                _ if request.starts_with("GET /users/") => handle_get_user_request(&request),
                _ if request.starts_with("GET /users") => handle_get_all_request(&request),
                _ if request.starts_with("POST /users") => handle_post_request(&request),
                _ if request.starts_with("PUT /users") => handle_update_request(&request),
                _ if request.starts_with("DELETE /users") => handle_delete_request(&request),

                _ => ("HTTP/1.1 404 NOT FOUND\r\n\r\n".to_owned(), "404 Not Found".to_owned()),
            };

            stream.write_all(format!("{}{}", status_line, content).as_bytes()).unwrap();
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

// Get one user
fn handle_get_user_request(request: &str) -> (String, String) {
    // Get the id from the request
    let id = get_id(&request);

    match id.parse::<i32>() {
        Ok(id_int) => {
            let mut client = Client::connect(DB_URL, NoTls).unwrap();
            match client.query_one("SELECT * FROM users WHERE id = $1", &[&id_int]) {
                Ok(row) => {
                    let user = User {
                        id: row.get(0),
                        name: row.get(1),
                        email: row.get(2),
                    };
                    let response_body = serde_json::to_string(&user).unwrap();
                    (
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(),
                        response_body,
                    )
                }
                Err(e) =>
                    (
                        "HTTP/1.1 404 NOT FOUND\r\n\r\n".to_owned(),
                        format!("User with ID {} not found", id),
                    ),
            }
        }
        Err(e) => ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), format!("Invalid ID: {}", id)),
    }
}

//Get all users
fn handle_get_all_request(_request: &str) -> (String, String) {
    // Connect to the database
    let mut client = Client::connect(DB_URL, NoTls).unwrap();

    let users: Vec<User> = client
        .query("SELECT id, name, email FROM users", &[])
        .unwrap()
        .into_iter()
        .map(|row| User {
            id: row.get(0),
            name: row.get(1),
            email: row.get(2),
        })
        .collect();

    let response_body = serde_json::to_string(&users).unwrap();
    ("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(), response_body)
}

//Create a new user
fn handle_post_request(request: &str) -> (String, String) {
    match deserialize_user_from_request_body(&request) {
        Ok(user) => {
            let mut client = Client::connect(DB_URL, NoTls).unwrap();
            if
                let Err(_) = client.execute(
                    "INSERT INTO users (name, email) VALUES ($1, $2)",
                    &[&user.name, &user.email]
                )
            {
                return (
                    "HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\n".to_owned(),
                    "Failed to insert user into database".to_owned(),
                );
            }

            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(),
                request.split("\r\n\r\n").last().unwrap_or("").to_string(),
            )
        }
        Err(_) =>
            ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), "Invalid request body".to_owned()),
    }
}

// Update user
fn handle_update_request(request: &str) -> (String, String) {
    // Get the id from the request
    let id = get_id(&request);

    // Deserialize the JSON body into a NewUser struct.
    let request_body = request.split("\r\n\r\n").last().unwrap_or("");
    let user: Result<NewUser, _> = serde_json::from_str(request_body);

    match user {
        Ok(new_user) => {
            let id_int = id.parse::<i32>();
            match id_int {
                Ok(id_int) => {
                    let mut client = Client::connect(DB_URL, NoTls).unwrap();
                    match
                        client.execute(
                            "UPDATE users SET name=$2, email=$3 WHERE id=$1",
                            &[&id_int, &new_user.name, &new_user.email]
                        )
                    {
                        Ok(_) =>
                            (
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(),
                                serde_json::to_string(&new_user).unwrap(),
                            ),
                        Err(e) =>
                            (
                                "HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\n".to_owned(),
                                format!("Error updating user: {}", e),
                            ),
                    }
                }
                Err(e) =>
                    (
                        "HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(),
                        format!("Invalid ID: {}. Error: {}", id, e),
                    ),
            }
        }
        Err(_) =>
            ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), "Invalid request body".to_owned()),
    }
}

// Delete user
fn handle_delete_request(request: &str) -> (String, String) {
    // Get the id from the request
    let id = get_id(&request);

    if let Ok(id_int) = id.parse::<i32>() {
        // Connect to the database.
        let mut client = Client::connect(DB_URL, NoTls).unwrap();
        let rows_affected = client.execute("DELETE FROM users WHERE id = $1", &[&id_int]).unwrap();

        // Return the appropriate response.
        if rows_affected == 1 {
            let response_body = serde_json::to_string(&id).unwrap();
            ("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(), response_body)
        } else {
            (
                "HTTP/1.1 404 NOT FOUND\r\n\r\n".to_owned(),
                format!("User with ID {} not found", id_int),
            )
        }
    } else {
        ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), format!("Invalid ID: {}", id))
    }
}

fn get_id(request: &str) -> &str {
    request.split('/').nth(2).unwrap_or_default().split_whitespace().next().unwrap_or_default()
}

fn deserialize_user_from_request_body(request: &str) -> Result<NewUser, serde_json::Error> {
    let request_body = request.split("\r\n\r\n").last().unwrap_or("");
    let user: Result<NewUser, _> = serde_json::from_str(request_body);
    user
}