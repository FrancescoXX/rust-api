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

// Environment variables defined in the docker compose to connect ot the DB
const DB_URL: &'static str = env!("DATABASE_URL");

fn main() {
    match set_database() {
        Ok(_) => (),
        Err(_) => (),
    }

    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_client(stream),
            Err(e) => println!("PostgresError: {}", e),
        }
    }
}

// Database setup: change this accordingly to the model
fn set_database() -> Result<(), PostgresError> {
    let mut client = Client::connect(DB_URL, NoTls).unwrap();

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

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 1024];

    match stream.read(&mut buffer) {
        Ok(size) => {
            let request = String::from_utf8_lossy(&buffer[..size]);

            let (status_line, content) = if request.starts_with("GET /users/") {
                handle_get_user_request(&request)
            } else if request.starts_with("POST /users") {
                handle_post_request(&request)
            } else if request.starts_with("GET /users") {
                ("HTTP/1.1 200 OK\r\n\r\n".to_owned(), handle_get_all_request(&request))
            } else if request.starts_with("GET /hello") {
                ("HTTP/1.1 200 OK\r\n\r\n".to_owned(), "Hello world".to_owned())
            } else if request.starts_with("DELETE /users") {
                handle_delete_request(&request)
            } else if request.starts_with("PUT /users") {
                handle_update_request(&request)
            } else {
                println!("Request: {}", request);
                ("HTTP/1.1 404 NOT FOUND\r\n\r\n".to_owned(), "404 Not Found".to_owned())
            };

            let response = format!("{}{}", status_line, content);
            stream.write(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}

// Get one user
fn handle_get_user_request(request: &str) -> (String, String) {
    let id_str = request.split('/').nth(2).unwrap_or("");
    let id = id_str.split_whitespace().next().unwrap_or("");
    if let Ok(id_int) = id.parse::<i32>() {
        let id_str = id_int.to_string();
        match find_user_by_id(&id_str) {
            Ok(user) => {
                let response_body = serde_json::to_string(&user).unwrap();
                (
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".to_owned(),
                    response_body,
                )
            }
            Err(e) => {
                ("HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\n".to_owned(), format!("Error: {}", e))
            }
        }
    } else {
        ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), format!("Invalid ID: {}", id_str))
    }
}

fn find_user_by_id(id: &str) -> Result<User, PostgresError> {
    let id_int = id.parse::<i32>().unwrap();
    let mut client = Client::connect(DB_URL, NoTls)?;
    let row = client.query_one("SELECT * FROM users WHERE id = $1", &[&id_int])?;

    Ok(User {
        id: row.get(0),
        name: row.get(1),
        email: row.get(2),
    })
}

//Update user
fn handle_update_request(request: &str) -> (String, String) {
    let request_body = request.split("\r\n\r\n").last().unwrap_or("");
    let user: User = serde_json::from_str(request_body).unwrap();
    let mut client = Client::connect(DB_URL, NoTls).unwrap();

    let id = request
        .split(" ")
        .nth(1)
        .and_then(|url| url.split("?").nth(1))
        .and_then(|params| params.split("=").nth(1))
        .and_then(|id_str| id_str.parse::<i32>().ok())
        .expect("Failed to parse ID");

    match client.execute("UPDATE users SET name=$2, email=$3 WHERE id=$1", &[&id, &user.name, &user.email]) {
        Ok(_) => ("HTTP/1.1 200 OK\r\n\r\n".to_owned(), format!("Updated user")),
        Err(e) => ("HTTP/1.1 500 Internal Server Error\r\n\r\n".to_owned(), format!("Error updating user: {}", e)),
    }
}

// Delete user
fn handle_delete_request(request: &str) -> (String, String) {
    let mut client = Client::connect(DB_URL, NoTls).unwrap();

    let id = request
        .split(" ")
        .nth(1)
        .and_then(|url| url.split("?").nth(1))
        .and_then(|params| params.split("=").nth(1))
        .and_then(|id_str| id_str.parse::<i32>().ok())
        .expect("Failed to parse ID");

    match client.execute("DELETE FROM users WHERE id=$1", &[&id]) {
        Ok(_) => ("HTTP/1.1 200 OK\r\n\r\n".to_owned(), format!("Deleted user")),
        Err(e) => (
            "HTTP/1.1 500 Internal Server Error\r\n\r\n".to_owned(),
            format!("Error deleting user: {}", e),
        ),
    }
}


//Get all users
fn handle_get_all_request(_request: &str) -> String {
    let mut client = Client::connect(DB_URL, NoTls).unwrap();
    let mut users: Vec<User> = Vec::new();

    for row in client.query("SELECT id, name, email FROM users", &[]).unwrap() {
        let id: i32 = row.get(0);
        let name: String = row.get(1);
        let email: String = row.get(2);

        let user = User {
            id: id,
            name: name,
            email: email,
        };

        users.push(user);
    }

    let users_json = serde_json::to_string(&users).unwrap();
    users_json
}

//Create a new user
fn handle_post_request(request: &str) -> (String, String) {
    let request_body = request.split("\r\n\r\n").last().unwrap_or("");

    #[derive(Serialize, Deserialize, Debug)]
    struct NewUser {
        pub name: String,
        pub email: String,
    }

    let user: NewUser = match serde_json::from_str(request_body) {
        Ok(user) => user,
        Err(_) => return ("HTTP/1.1 400 BAD REQUEST\r\n\r\n".to_owned(), "Invalid request body".to_owned()),
    };

    let mut client = Client::connect(DB_URL, NoTls).unwrap();
    if let Err(_) = client.execute("INSERT INTO users (name, email) VALUES ($1, $2)", &[&user.name, &user.email]) {
        return ("HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\n".to_owned(), "Failed to insert user into database".to_owned());
    }

    ("HTTP/1.1 200 OK\r\n\r\n".to_owned(), format!("Received data: {}", request_body))
}
