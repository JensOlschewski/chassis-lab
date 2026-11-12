function [dfiComp, dfjComp] = calculateConForces(conStruct, vhat, numJoints)
    % ##################################################################################
    % calculateConForces
    %
    % This function calculates the force gradients at connector points in a
    % mechanical system, using kinematic analysis to evaluate velocity differences
    % and compute the corresponding forces. It iterates over all connection elements,
    % applies the stiffness matrix to determine force gradients, and transforms these
    % forces to the connector coordinate frames.
    %
    % Inputs:
    %   conStruct - Structure containing connector parameters, including body IDs,
    %               joint IDs, stiffness matrices, and transformation matrices
    %   vhat      - Global velocity vector for all bodies in the system
    %   h         - Step size for the analysis (not used in this function)
    %   numJoints - Total number of joints in the system
    %
    % Outputs:
    %   dfiComp   - Compiled force vector at side I of each joint (6x1 per joint)
    %   dfjComp   - Compiled force vector at side J of each joint (6x1 per joint)
    % ##################################################################################
    

    % Initialitze empty con force arrays 
    dfiComp = zeros(numJoints*6,1);
    dfjComp = zeros(numJoints*6,1);

    
    % Iterate over each con element in the structure
    for i = 1:numel(conStruct)

        if isfield(conStruct(i), 'active') && conStruct(i).active
            
            % Extract body IDs
            ibodyID = conStruct(i).i.bodyID;
            jbodyID = conStruct(i).j.bodyID;
            jointID = conStruct(i).jointID;

            % Initialize empty velocity vectors
            vi = zeros(6,1);
            vj = zeros(6,1);

            % Extract values from vhat based on body IDs
            if ibodyID ~= 0 
                iRowIndices = (ibodyID * 6 - 5):ibodyID * 6;
                vi = vhat(iRowIndices);
            end
            if jbodyID ~= 0
                jRowIndices = (jbodyID * 6 - 5):jbodyID * 6;
                vj = vhat(jRowIndices);
            end

            % Check if the connection is from i to j or j to i
            if conStruct(i).isJGreaterThanI
                % Calculate force gradient at the con
                dfeAtCM = conStruct(i).charStruct.kMatrixCoup * [vi; vj];

                % Extract for each side and tranform to con point
                dfiComp(jointID*6-5:jointID*6) = conStruct(i).Ti' * dfeAtCM(1:6);
                dfjComp(jointID*6-5:jointID*6) = conStruct(i).Tj' * dfeAtCM(7:12);
                
                
            else
                % Calculate force gradient at the con
                dfeAtCM = conStruct(i).charStruct.kMatrixCoup  * [vj; vi];
                
                % Extract for each side and tranform to con point
                dfiComp(jointID*6-5:jointID*6) = conStruct(i).Ti' * -dfeAtCM(7:12);
                dfjComp(jointID*6-5:jointID*6) = conStruct(i).Tj' * -dfeAtCM(1:6);

            end              
        end
    end
end
