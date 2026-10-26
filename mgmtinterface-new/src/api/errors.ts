/**
 * The errors `apiRequestV2` throws. Loaders and screens catch these rather than
 * looking at responses: `Unauthorized` sends the user to /login, and any other
 * `ApiError` carries the API's own message, written to be shown to the user.
 */

export class ApiError extends Error {
    readonly status: number

    constructor(status: number, message: string) {
        super(message)
        this.name = 'ApiError'
        this.status = status
    }
}

export class Unauthorized extends ApiError {
    constructor(message = 'Unauthorized') {
        super(401, message)
        this.name = 'Unauthorized'
    }
}
