from subprocess import CalledProcessError
from typing import Dict, Iterable, List
from typing_extensions import TypedDict

from flask import Flask, request, jsonify
import jsonschema
import pamela

from permissions.api import AdhocProject, ValidationError, authenticate, list_users, id

app = Flask(__name__)


class InvalidAPIUsage(Exception):
    """
    Exception type for conveying the right status code to return
    """
    status_code = 400

    def __init__(self, message, status_code=None, payload=None):
        super().__init__()
        self.message = message
        if status_code is not None:
            self.status_code = status_code
        self.payload = payload

    def to_dict(self):
        rv = dict(self.payload or ())
        rv['message'] = self.message
        rv['errno'] = 1
        return rv


@app.errorhandler(InvalidAPIUsage)
def invalid_api_usage(e):
    return jsonify(e.to_dict()), e.status_code


AUTHENTICATE_USER_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/authenticate-user-request.schema.json",
    "title": "AuthenticateUserRequest",
    "description": "The JSON payload for the POST request to authenticate a user",
    "type": "object",
    "properties": {
        "username": {
            "type": "string",
            "description": "Username",
        },
        "password": {
            "type": "string",
            "description": "Password",
        },
    },
    "required": ["username", "password"],
}


class AuthenticateUserRequest(TypedDict):
    username: str
    password: str


@app.route("/users/auth/", methods=["POST"])
def authenticate_user() -> dict:
    request_data: AuthenticateUserRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=AUTHENTICATE_USER_REQUEST_SCHEMA, instance=request_data
        )
        try:
            authenticate(request_data['username'], request_data['password'])
            return {
                'errno': 0,
                'id': id(request_data['username']),
            }
        except pamela.PAMError as e:
            return {
                'errno': e.errno,
                'message': e.message
            }
    except ValidationError as e:
        raise InvalidAPIUsage(str(e))
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/users/id/<name>/", methods=["GET"])
def id_user(name) -> dict:
    return {
        'errno': 0,
        'id': id(name),
    }


@app.route("/users/list/", methods=["GET"])
def get_user_list() -> Dict:
    return {
        'errno': 0,
        "users": list_users(),
    }


CREATE_PROJECT_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to create a new project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Short name of the project",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the project",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 0,
            "uniqueItems": True,
            "description": "Usernames of the members of this ad-hoc group",
        },
    },
    "required": ["name", "owner", "members"],
}


class CreateProjectRequest(TypedDict):
    name: str
    owner: str
    members: List[str]


@app.route("/project/", methods=["POST"])
def create_project() -> Dict:
    request_data: CreateProjectRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data
        )
        project = AdhocProject.create(
            name=request_data['name'],
            owner=request_data['owner'],
            members=request_data['members']
        )
        return {
            'errno': 0,
            'project': project.serialize(),
        }
    except (ValidationError, KeyError) as e:
        return {
            'errno': 1,
            'message': str(e)
        }
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved


@app.route("/project/", methods=["GET"])
def list_projects() -> Dict:
    # TODO: catch exceptions? OSError?
    project_names = AdhocProject.list_all()
    return {
        'errno': 0,
        'projects': [
            AdhocProject(project_name).serialize(fast=True)
            for project_name in project_names
        ]
    }


@app.route("/project/<project_name>/", methods=["GET"])
def project_details(project_name) -> Dict:
    try:
        p = AdhocProject(project_name)
    except ValidationError as e:
        return{
            'errno': 1,
            'message': str(e)
        }

    # TODO: catch exceptions? `p.datasets` can raise multiple...
    return {
        'errno': 0,
        "project": p.serialize(),
        "datasets": [
            {
                "name": ds.name,
                "immutable": ds.immutable,
                "ctime": ds.ctime,
                "mtime": ds.mtime,
            }
            for ds in p.datasets
        ]
    }


CREATE_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-dataset-request.schema.json",
    "title": "CreateDatasetRequest",
    "description": "The JSON payload for the POST request to create a new dataset in a project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset, which will be used as directory name",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the dataset",
        },
    },
    "required": ["name", "owner"],
}


class CreateDatasetRequest(TypedDict):
    name: str
    owner: str


@app.route("/project/<project_name>/dataset/", methods=["POST"])
def create_dataset(project_name) -> Dict:
    request_data: CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=CREATE_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        p = AdhocProject(project_name)
    except ValidationError as e:
        return{
            'errno': 1,
            'message': str(e)
        }
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    # TODO: catch exceptions? for example, `CalledProcessError` if `mkdir` fails...
    try:
        p.add_dataset(
            name=request_data["name"],
            user=request_data["owner"]
        )
        return {
            "errno": 0
        }  # what should be returned here?
    except CalledProcessError as e:
        return {
            'errno': e.returncode,
            'message': e.stderr
        }
    except ValidationError as e:
        return {
            'errno': 1,
            'message': str(e)
        }



SEAL_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/seal-dataset-request.schema.json",
    "title": "SealDatasetRequest",
    "description": "The JSON payload for the POST request to seal a dataset",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset",
        },
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["name", "user"],
}


class SealDatasetRequest(TypedDict):
    name: str
    user: str


# NOTE: not using data set name in path, as it can contain slashes (right?) so
# we are using a json payload for this request, which may seem a bit like
# overkill....
@app.route("/project/<project_name>/dataset/seal/", methods=["POST"])
def seal_dataset(project_name):
    request_data: CreateDatasetRequest = request.json  # type:ignore
    try:
        jsonschema.validate(
            schema=SEAL_DATASET_REQUEST_SCHEMA, instance=request_data
        )
        # TODO: catch exceptions?
        p = AdhocProject(project_name)
        p.seal_dataset(request_data["name"], request_data["user"])
    except ValidationError as e:
        return{
            'errno': 1,
            'message': str(e)
        }
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {
        'errno': 0,
    }  # what should be returned here?


CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://permissions.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to change membership of a project",
    "type": "object",
    "properties": {
        "op": {
            "type": "string",
            "enum": ["add", "remove"],
            "description": "Operation: add or remove",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 1,
            "uniqueItems": True,
            "description": "Usernames of the members to add or remove from this group",
        },
        "user": {
            "type": "string",
            "description": "Username of the requesting user",
        },
    },
    "required": ["op", "members", "user"],
}


class ChangeMembersRequest(TypedDict):
    op: str
    members: List[str]
    user: str


@app.route("/project/<project_name>/members/", methods=["PATCH"])
def change_project_members(project_name):
    request_data: ChangeMembersRequest = request.json  # type:ignore
    # TODO: catch exceptions from `add_members` and `remove_members`
    try:
        jsonschema.validate(
            schema=CHANGE_PROJECT_MEMBERS_REQUEST_SCHEMA, instance=request_data
        )
        p = AdhocProject(project_name)
        op = request_data["op"]
        if op == "add":
            p.add_members(request_data["members"], request_data['user'])
        elif op == "remove":
            p.remove_members(request_data["members"], request_data['user'])
        else:
            raise InvalidAPIUsage(f"Unknown op: {op}")
    except (ValidationError, KeyError) as e:
        return{
            'errno': 1,
            'message': str(e)
        }
    except jsonschema.exceptions.ValidationError as e:
        raise InvalidAPIUsage(str(e))  # message can possibly be improved
    return {
        'errno': 0,
        "project": p.serialize()
    }
