import time
import osprey

while True:
    print("I'm running!")

    try:
        osprey.main()
    except Exception as e:
        print(f"Error running osprey: {e}")

    # sleep for 24 hours
    time.sleep(24 * 60 * 60)
