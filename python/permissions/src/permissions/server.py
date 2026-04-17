from subprocess import CalledProcessError

from flask import Flask, request, jsonify
import jsonschema
import jsonschema.exceptions
import pamela

from permissions.api import AdhocProject, ValidationError, authenticate, list_users, uid
from permissions_client.schema import (
    AuthenticateResponse,
    ChangeProjectMembersResponse,
    CreateDatasetResponse,
    CreateProjectResponse,
    GenericErrorResponse,
    GetUserListResponse,
    IdUserResponse,
    InvalidAPIUsage,
    AUTHENTICATE_USER_REQUEST_SCHEMA,
    AuthenticateUserRequest,
    CREATE_PROJECT_REQUEST_SCHEMA,
    CreateProjectRequest,
    CREATE_DATASET_REQUEST_SCHEMA,
    CreateDatasetRequest,
    SEAL_DATASET_REQUEST_SCHEMA,
    ListProjectsResponse,
    ProjectDetailsResponse,
    SealDatasetRequest,
    CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA,
    ChangeMembersRequest,
    SealDatasetResponse,
)

app = Flask(__name__)

app.config.from_object("permissions.default_config")
app.config.from_envvar("PERMISSIONS_CONFIG")


@app.errorhandler(InvalidAPIUsage)
def invalid_api_usage(e):
    return jsonify(e.to_dict()), e.status_code


@app.route("/users/auth/", methods=["POST"])
def authenticate_user() -> AuthenticateResponse:
    request_data: AuthenticateUserRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(
            schema=AUTHENTICATE_USER_REQUEST_SCHEMA, instance=request_data
        )
        try:
            authenticate(request_data["username"], request_data["password"])
            return {
                "errno": 0,
                "id": uid(request_data["username"]),
            }
        except pamela.PAMError as e:
            return {"errno": e.errno, "message": e.message}
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/users/id/<name>/", methods=["GET"])
def id_user(name: str) -> IdUserResponse:
    return {
        "errno": 0,
        "id": uid(name),
    }


@app.route("/users/list/", methods=["GET"])
def get_user_list() -> GetUserListResponse:
    return {
        "errno": 0,
        "users": list_users(app.config["ADHOC_GROUP"]),
    }


@app.route("/project/", methods=["POST"])
def create_project() -> CreateProjectResponse | GenericErrorResponse:
    request_data: CreateProjectRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(schema=CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data)
        project = AdhocProject.create(
            name=request_data["name"],
            owner=request_data["owner"],
            members=request_data["members"],
            adhoc_group=app.config["ADHOC_GROUP"],
        )
        return {
            "errno": 0,
            "project": project.serialize(),
        }
    except (ValidationError, KeyError) as e:
        return {"errno": 1, "message": str(e)}
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/project/", methods=["GET"])
def list_projects() -> ListProjectsResponse:
    # TODO: catch exceptions? OSError?
    project_names = AdhocProject.list_all()
    return {
        "errno": 0,
        "projects": [
            AdhocProject(project_name, adhoc_group=app.config["ADHOC_GROUP"]).serialize(
                fast=True
            )
            for project_name in project_names
        ],
    }


@app.route("/project/<project_name>/", methods=["GET"])
def project_details(project_name: str) -> ProjectDetailsResponse | GenericErrorResponse:
    fast = request.args.get("fast", default=False, type=bool)
    try:
        p = AdhocProject(project_name, adhoc_group=app.config["ADHOC_GROUP"])
    except ValidationError as e:
        return {"errno": 1, "message": str(e)}

    # TODO: catch exceptions? `p.datasets` can raise multiple...
    return {
        "errno": 0,
        "project": p.serialize(fast=fast),
        "datasets": [
            {
                "name": ds.name,
                "immutable": ds.immutable,
                "ctime": ds.ctime,
                "mtime": ds.mtime,
            }
            for ds in p.datasets
        ],
    }


@app.route("/project/<project_name>/dataset/", methods=["POST"])
def create_dataset(project_name: str) -> CreateDatasetResponse | GenericErrorResponse:
    request_data: CreateDatasetRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(schema=CREATE_DATASET_REQUEST_SCHEMA, instance=request_data)
        p = AdhocProject(project_name, adhoc_group=app.config["ADHOC_GROUP"])
        p.add_dataset(name=request_data["name"], user=request_data["owner"])
        return {"errno": 0}  # what should be returned here?
    except CalledProcessError as e:
        return {"errno": e.returncode, "message": e.stderr}
    except ValidationError as e:
        return {"errno": 1, "message": str(e)}
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


# NOTE: not using data set name in path, as it can contain slashes (right?) so
# we are using a json payload for this request, which may seem a bit like
# overkill....
@app.route("/project/<project_name>/dataset/seal/", methods=["POST"])
def seal_dataset(project_name: str) -> SealDatasetResponse | GenericErrorResponse:
    request_data: SealDatasetRequest = request.json  # pyright: ignore[reportAssignmentType]
    try:
        jsonschema.validate(schema=SEAL_DATASET_REQUEST_SCHEMA, instance=request_data)
        # TODO: catch exceptions?
        p = AdhocProject(project_name, adhoc_group=app.config["ADHOC_GROUP"])
        p.seal_dataset(request_data["name"], request_data["user"])
    except ValidationError as e:
        return {"errno": 1, "message": str(e)}
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {
        "errno": 0,
    }  # what should be returned here?


@app.route("/project/<project_name>/members/", methods=["PATCH"])
def change_project_members(
    project_name: str,
) -> ChangeProjectMembersResponse | GenericErrorResponse:
    request_data: ChangeMembersRequest = request.json  # pyright: ignore[reportAssignmentType]
    # TODO: catch exceptions from `add_members` and `remove_members`
    try:
        jsonschema.validate(
            schema=CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA, instance=request_data
        )
        p = AdhocProject(project_name, adhoc_group=app.config["ADHOC_GROUP"])
        op = request_data["op"]
        if op == "add":
            p.add_members(request_data["members"], request_data["user"])
        elif op == "remove":
            p.remove_members(request_data["members"], request_data["user"])
        else:
            raise InvalidAPIUsage(f"Unknown op: {op}")
    except (ValidationError, KeyError) as e:
        return {"errno": 1, "message": str(e)}
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {"errno": 0, "project": p.serialize()}
