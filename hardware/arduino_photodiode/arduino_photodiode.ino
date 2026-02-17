/**
 * OpenISI Photodiode Timestamp Capture
 *
 * Detects light changes on a display sync patch and sends hardware
 * timestamps over USB serial. Used for true hardware vsync timing
 * when software-only solutions aren't available (e.g., macOS Apple Silicon).
 *
 * Hardware:
 *   - Arduino Uno/Mega (or compatible)
 *   - BPW34 photodiode (or similar silicon photodiode)
 *   - 390kΩ resistor
 *
 * Circuit:
 *       VCC (5V)
 *         │
 *         ├──[390kΩ]──┬── Analog Pin A0
 *         │           │
 *         │       [BPW34]
 *         │       (photodiode, cathode to GND)
 *         │           │
 *         └───────────┴── GND
 *
 * Protocol (OpenISI Photodiode Protocol):
 *   Device → Host:
 *     T<us>\n     - Timestamp when light detected (microseconds)
 *     S<code>\n   - Status (0=ok, 1=error)
 *     I<text>\n   - Device info
 *
 *   Host → Device:
 *     P\n         - Ping (request timestamp for clock sync)
 *     H<val>\n    - Set threshold (0-1023)
 *     R\n         - Reset
 *
 * Author: OpenISI Contributors
 * License: MIT
 */

// Configuration
const int PHOTODIODE_PIN = A0;
const int LED_PIN = 13;  // Built-in LED for status

// Default threshold (adjust based on your setup)
// Higher = less sensitive, Lower = more sensitive
int threshold = 512;

// Hysteresis to prevent oscillation
const int HYSTERESIS = 50;

// Minimum time between events (debounce)
const unsigned long DEBOUNCE_US = 8000;  // 8ms = ~120Hz max

// State
volatile bool triggered = false;
volatile unsigned long lastTriggerUs = 0;

// Version
const char* VERSION = "OpenISI-Photodiode v1.0";

void setup() {
    // Initialize serial
    Serial.begin(115200);
    while (!Serial) {
        ; // Wait for serial port (needed for some boards)
    }

    // Configure pins
    pinMode(PHOTODIODE_PIN, INPUT);
    pinMode(LED_PIN, OUTPUT);
    digitalWrite(LED_PIN, LOW);

    // Send device info
    Serial.print("I");
    Serial.println(VERSION);

    // Send ready status
    Serial.println("S0");
}

void loop() {
    // Check for incoming commands
    handleSerial();

    // Read photodiode
    int reading = analogRead(PHOTODIODE_PIN);
    unsigned long now = micros();

    // Rising edge detection with debounce
    if (!triggered && reading > threshold) {
        if ((now - lastTriggerUs) > DEBOUNCE_US) {
            triggered = true;
            lastTriggerUs = now;

            // Send timestamp
            Serial.print("T");
            Serial.println(now);

            // Flash LED
            digitalWrite(LED_PIN, HIGH);
        }
    }

    // Falling edge (with hysteresis)
    if (triggered && reading < (threshold - HYSTERESIS)) {
        triggered = false;
        digitalWrite(LED_PIN, LOW);
    }
}

void handleSerial() {
    if (Serial.available() == 0) {
        return;
    }

    char cmd = Serial.read();

    switch (cmd) {
        case 'P':
            // Ping - respond with current timestamp
            Serial.print("T");
            Serial.println(micros());
            break;

        case 'H':
            // Set threshold
            {
                int val = Serial.parseInt();
                if (val >= 0 && val <= 1023) {
                    threshold = val;
                    Serial.println("S0");  // OK
                } else {
                    Serial.println("S1");  // Error
                }
            }
            break;

        case 'R':
            // Reset
            triggered = false;
            lastTriggerUs = 0;
            Serial.println("S0");
            break;

        case '\n':
        case '\r':
            // Ignore line endings
            break;

        default:
            // Unknown command
            break;
    }

    // Flush any remaining input
    while (Serial.available() > 0 && Serial.peek() == '\n') {
        Serial.read();
    }
}
