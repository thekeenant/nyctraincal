# NYC Train Cal

http://nyctraincal.keenant.com

An HTTP server that provides real-time MTA subway alerts as iCalendar (.ics) files, filterable by train line.

## Features

- Fetches real-time MTA subway alerts from the official MTA API
- Serves alerts as iCalendar (.ics) files that can be subscribed to in any calendar application
- Filter alerts by specific train lines
- Converts MTA alert data to calendar events with proper time ranges

## Running the Server

```bash
cargo run
```

The server will start on `http://0.0.0.0:3000`

## API Endpoints

### Get Calendar for a Specific Train Line

```
GET /api/calendars/train/<train_name>.ics
```

**Examples:**
- `http://localhost:3000/api/calendars/train/1.ics` - Get alerts for the 1 train
- `http://localhost:3000/api/calendars/train/A.ics` - Get alerts for the A train
- `http://localhost:3000/api/calendars/train/Q.ics` - Get alerts for the Q train

The `.ics` extension is optional - both `/train/A.ics` and `/train/A` work.

## Subscribing to Calendars

You can subscribe to these calendars in any calendar application that supports iCalendar subscriptions:

### Google Calendar
1. Copy the calendar URL (e.g., `http://your-domain.com/api/calendars/train/A.ics`)
2. In Google Calendar, click the + next to "Other calendars"
3. Select "From URL"
4. Paste the URL and click "Add calendar"

### Apple Calendar
1. File → New Calendar Subscription
2. Enter the calendar URL
3. Click Subscribe

### Outlook
1. Add calendar → From internet
2. Enter the calendar URL
3. Click Import
