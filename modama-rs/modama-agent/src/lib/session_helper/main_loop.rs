pub async fn session_helper_loop(
    // websocket here
) {
    // connect to the internal websocket API
    // receive messages
    // add a timeout when receiving, so we reconcile periodically
    // -> we need to keep the latest received desired state around for periodic reconciliation
    // handle each message by calling the reconciliation function(s)
}
